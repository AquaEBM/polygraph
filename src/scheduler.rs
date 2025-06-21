use super::*;

/// Inserts a key-value pair into a map
///
/// # Panics
///
/// if `map.contains_key(&k)`
fn insert_new<K: Hash + Eq, V>(map: &mut HashMap<K, V>, k: K, v: V) {
    assert!(map.insert(k, v).is_none());
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SourceType {
    Direct { delay: u64 },
    Sum { index: usize },
}

impl Debug for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct { delay } => write!(f, "Direct {{ delay: {delay:?} }}"),
            Self::Sum { index } => write!(f, "Sum {{ index: {index:?} }}"),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct InputBufferAssignment {
    pub node: NodeID,
    pub port: OutputID,
    pub kind: SourceType,
}

impl InputBufferAssignment {
    #[inline]
    #[must_use]
    pub fn incoming_delay(&self) -> u64 {
        match &self.kind {
            SourceType::Direct { delay } => *delay,
            SourceType::Sum { .. } => 0,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct SumTask {
    pub rhs_delay: u64,
    pub lhs: InputBufferAssignment,
    pub output: u32,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct OutputBufferAssignment<T = u64> {
    id: u32,
    max_delay: T,
    sum_tasks: Box<[SumTask]>,
}

#[derive(PartialEq, Eq, Clone, Default, Debug)]
pub struct NodeIO<T = u64> {
    inputs: HashMap<InputID, InputBufferAssignment>,
    outputs: HashMap<OutputID, OutputBufferAssignment<T>>,
}

#[derive(Debug, Default)]
pub(crate) struct BufferAllocator {
    ids: Vec<Rc<()>>,
}

impl BufferAllocator {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    #[inline]
    fn find_free_buffer(&mut self) -> (u32, &Rc<()>) {
        let id = self
            .ids
            .iter()
            .enumerate()
            .find(|(_, claims)| Rc::strong_count(claims) == 1)
            .map(|(id, _)| id)
            .unwrap_or_else(|| {
                let new_id = self.ids.len();
                self.ids.push(Rc::new(()));
                new_id
            });

        (
            u32::try_from(id).expect("more than u32::MAX buffers, aborting"),
            &self.ids[id],
        )
    }
}

#[derive(Debug, Clone)]
pub struct Scheduler<'a> {
    graph: &'a Graph,
    order: Vec<NodeID>,
    intermediate: HashMap<NodeID, (u64, HashMap<OutputID, Port<InputID>>)>,
}

impl Scheduler<'_> {
    #[must_use]
    pub fn intermediate(&self) -> &HashMap<NodeID, (u64, HashMap<OutputID, Port<InputID>>)> {
        &self.intermediate
    }

    #[must_use]
    pub fn order(&self) -> &[NodeID] {
        &self.order
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Task {
    Sum {
        node_id: NodeID,
        port_id: OutputID,
        index: usize,
    },
    Node(NodeID),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct GraphSchedule<T = u64> {
    pub num_buffers: usize,
    pub node_io: HashMap<NodeID, NodeIO<T>>,
    pub intermediate: HashMap<NodeID, (u64, HashMap<OutputID, Port<InputID>>)>,
    pub tasks: Box<[Task]>,
}

impl<'a> Scheduler<'a> {
    #[inline]
    pub(crate) fn for_graph(graph: &'a Graph) -> Self {
        Self {
            graph,
            intermediate: HashMap::default(),
            order: Vec::new(),
        }
    }

    pub fn add_sink_node(&mut self, index: &NodeID) {
        if self.intermediate.contains_key(index) {
            return;
        }

        let mut max_input_lat = 0;

        for (dest_port_id, dest_port) in self.graph[index].input_ports() {
            for (source_node_id, source_port_ids) in dest_port.connections() {
                self.add_sink_node(source_node_id);

                let (source_node_input_lat, source_node_outputs) =
                    self.intermediate.get_mut(source_node_id).unwrap();

                for source_port_id in source_port_ids {
                    source_node_outputs
                        .entry(*source_port_id)
                        .or_default()
                        .insert_connection(*index, *dest_port_id);

                    let source_port_lat =
                        self.graph[source_node_id].output_latencies()[source_port_id];
                    let source_port_total_lat = *source_node_input_lat + source_port_lat;

                    max_input_lat = max_input_lat.max(source_port_total_lat);
                }
            }
        }

        assert!(
            self.intermediate
                .insert(*index, (max_input_lat, HashMap::default()))
                .is_none()
        );
    }

    pub fn compile_map_delays<T>(self, mut f: impl FnMut(u64) -> T) -> GraphSchedule<T> {
        let mut allocator = BufferAllocator::default();

        let mut claims = HashMap::<_, HashMap<_, _>>::default();

        let mut node_io = HashMap::default();

        let mut tasks = vec![];

        let Self {
            graph,
            intermediate,
            order,
        } = self;

        for id in &order {
            let (&node_id, (max_input_lat, node_outputs)) = intermediate.get_key_value(id).unwrap();

            tasks.push(Task::Node(node_id));

            let node_output_lats = graph[&node_id].output_latencies();

            let mut inputs = HashMap::default();
            let mut outputs = HashMap::default();

            let mut repeat_assignees = HashMap::default();

            // for every (actually used) output of this node

            for (&source_port_id, source_port) in node_outputs {
                let connections = source_port.connections();
                if connections.is_empty() {
                    continue;
                }

                // allocate a buffer for it
                let (id, handle_ref) = allocator.find_free_buffer();

                let source_total_lat = max_input_lat + node_output_lats[&source_port_id];
                let mut max_delay = 0;

                let mut repeats = HashMap::default();

                for (&dest_node_id, dest_port_ids) in connections {
                    let mut prev_assigned_ports = HashMap::default();

                    // find the maximum delay it will be subjected to
                    let delay = intermediate[&dest_node_id].0 - source_total_lat;
                    max_delay = max_delay.max(delay);

                    // assign the buffer to all recieveing ports, and keep track of ports
                    // that have already been assigned a buffer
                    for &dest_port_id in dest_port_ids {
                        let handle = Rc::clone(handle_ref);

                        if claims
                            .get(&dest_node_id)
                            .is_some_and(|map| map.contains_key(&dest_port_id))
                        {
                            prev_assigned_ports.insert(dest_port_id, (handle, delay));
                        } else {
                            claims.entry(dest_node_id).or_default().insert(
                                dest_port_id,
                                (
                                    handle,
                                    InputBufferAssignment {
                                        node: node_id,
                                        port: source_port_id,
                                        kind: SourceType::Direct { delay },
                                    },
                                ),
                            );
                        }
                    }

                    repeats.insert(dest_node_id, prev_assigned_ports);
                }

                repeat_assignees.insert(source_port_id, repeats);

                outputs.insert(
                    source_port_id,
                    OutputBufferAssignment {
                        id,
                        max_delay: f(max_delay),
                        sum_tasks: Box::new([]),
                    },
                );
            }

            // handle repeat assignments
            for (port_id, repeat_assignees) in repeat_assignees {
                let mut sum_tasks = vec![];

                for (dest_node_id, repeat_assignees) in repeat_assignees {
                    let dest_node_claims = claims.get_mut(&dest_node_id).unwrap();

                    for (dest_port_id, (other_old_handle, delay)) in repeat_assignees {
                        // we're not using insert directly to allow dropping
                        // {this, other}_old_handle early

                        let (this_old_handle, lhs) =
                            dest_node_claims.remove(&dest_port_id).unwrap();

                        // because we can potentially reuse the input buffers if they have no latency
                        if delay == 0 {
                            drop(other_old_handle);
                        }

                        if lhs.incoming_delay() == 0 {
                            drop(this_old_handle);
                        }

                        let (output, new_handle_ref) = allocator.find_free_buffer();

                        let index = sum_tasks.len();

                        assert!(
                            dest_node_claims
                                .insert(
                                    dest_port_id,
                                    (
                                        Rc::clone(new_handle_ref),
                                        InputBufferAssignment {
                                            node: node_id,
                                            port: port_id,
                                            kind: SourceType::Sum { index },
                                        },
                                    ),
                                )
                                .is_none()
                        );

                        tasks.push(Task::Sum {
                            node_id,
                            port_id,
                            index,
                        });

                        sum_tasks.push(SumTask {
                            rhs_delay: delay,
                            lhs,
                            output,
                        });
                    }
                }

                outputs.get_mut(&port_id).unwrap().sum_tasks = sum_tasks.into_boxed_slice();
            }

            for (dest_port_id, (_handle, source)) in claims
                .get_mut(&node_id)
                .into_iter()
                .flat_map(HashMap::drain)
            {
                insert_new(&mut inputs, dest_port_id, source);
            }

            assert!(
                node_io
                    .insert(node_id, NodeIO { inputs, outputs })
                    .is_none()
            );
        }

        GraphSchedule {
            num_buffers: allocator.len(),
            node_io,
            intermediate,
            tasks: tasks.into_boxed_slice(),
        }
    }

    pub fn compile(self) -> GraphSchedule {
        self.compile_map_delays(convert::identity)
    }
}
