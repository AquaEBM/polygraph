use super::*;

/// Inserts a key-value pair into a map
///
/// # Panics
///
/// if `map.contains_key(&k)`
fn insert_new<K: Hash + Eq, V>(map: &mut HashMap<K, V>, k: K, v: V) {
    assert!(map.insert(k, v).is_none())
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct BufferID(NonZeroU32);

impl Debug for BufferID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BufferID({:?})", &self.0)
    }
}

impl BufferID {
    #[inline]
    pub(crate) fn new_key(map: &HashMap<Self, impl Sized>) -> Self {
        let mut id = Self(NonZeroU32::MIN);

        while map.contains_key(&id) {
            id.0 = id.0.checked_add(1).expect("Index overflow");
        }

        id
    }
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
    node: NodeID,
    port: OutputID,
    kind: SourceType,
}

impl InputBufferAssignment {
    fn incoming_delay(&self) -> u64 {
        match &self.kind {
            SourceType::Direct { delay } => *delay,
            _ => 0,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct SumTask {
    rhs_delay: u64,
    lhs: InputBufferAssignment,
    output: BufferID,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct OutputBufferAssignment {
    id: BufferID,
    max_delay: u64,
    sum_tasks: Box<[SumTask]>,
}

#[derive(PartialEq, Eq, Clone, Default, Debug)]
pub struct ScheduleEntry {
    inputs: HashMap<InputID, InputBufferAssignment>,
    outputs: IndexMap<OutputID, OutputBufferAssignment>,
}

#[derive(Debug, Default)]
pub(crate) struct BufferAllocator {
    ids: HashMap<BufferID, Rc<()>>,
}

impl BufferAllocator {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    #[inline]
    fn find_free_buffer(&mut self) -> (&BufferID, &Rc<()>) {
        let key = self
            .ids
            .iter()
            .find(|(_, claims)| Rc::strong_count(claims) == 1)
            .map(|(&id, _)| id)
            .unwrap_or_else(|| {
                let new_key = BufferID::new_key(&self.ids);
                insert_new(&mut self.ids, new_key, Default::default());
                new_key
            });

        self.ids.get_key_value(&key).unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct Scheduler<'a> {
    graph: &'a Graph,
    intermediate: HashMap<NodeID, HashMap<OutputID, Port<InputID>>>,
    max_input_lats: IndexMap<NodeID, u64>,
}

impl<'a> Scheduler<'a> {
    pub fn max_input_lats(&self) -> &IndexMap<NodeID, u64> {
        &self.max_input_lats
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct GraphSchedule {
    pub num_buffers: usize,
    pub tasks: HashMap<NodeID, ScheduleEntry>,
}

impl<'a> Scheduler<'a> {
    #[inline]
    pub(crate) fn for_graph(graph: &'a Graph) -> Self {
        Self {
            graph,
            intermediate: Default::default(),
            max_input_lats: Default::default(),
        }
    }

    pub fn add_sink_node(&mut self, index: &NodeID) {
        if self.max_input_lats.contains_key(index) {
            return;
        }

        self.intermediate.entry(*index).or_default();

        let mut max_input_lat = 0;

        for (dest_port_id, dest_port) in self.graph[index].input_ports() {
            for (source_node_id, source_port_ids) in dest_port.connections() {
                self.add_sink_node(source_node_id);
                let source_node_input_lat = &self.max_input_lats[source_node_id];

                let source_node_outputs = self.intermediate.entry(*source_node_id).or_default();

                for source_port_id in source_port_ids {
                    source_node_outputs
                        .entry(*source_port_id)
                        .or_default()
                        .insert_connection(*index, *dest_port_id);

                    let source_port_lat =
                        self.graph[source_node_id].output_latencies()[source_port_id];
                    let source_port_total_lat = source_node_input_lat + source_port_lat;

                    max_input_lat = max_input_lat.max(source_port_total_lat);
                }
            }
        }

        assert!(self.max_input_lats.insert(*index, max_input_lat).is_none());
    }

    pub fn compile(&self) -> GraphSchedule {
        let mut allocator = BufferAllocator::default();

        let mut claims = HashMap::<_, HashMap<_, _>>::default();

        let mut tasks = HashMap::default();

        let Self {
            graph,
            intermediate,
            max_input_lats,
        } = self;

        for (&node_id, max_input_lat) in max_input_lats {
            let current_node = &graph[&node_id];
            let node_output_lats = current_node.output_latencies();

            let mut inputs = HashMap::default();
            let mut outputs = IndexMap::default();

            let mut repeat_assignees = HashMap::default();

            // for every (actually used) output of this node

            for (&source_port_id, source_port) in intermediate[&node_id].iter() {
                let connections = source_port.connections();
                if connections.is_empty() {
                    continue;
                }

                // allocate a buffer for it
                let (&id, handle_ref) = allocator.find_free_buffer();

                let source_total_lat = max_input_lat + node_output_lats[&source_port_id];
                let mut max_delay = 0;

                let mut repeats = HashMap::default();

                for (&dest_node_id, dest_port_ids) in connections {
                    let mut prev_assigned_ports = HashMap::default();

                    // find the maximum delay it will be subjected to
                    let delay = max_input_lats[&dest_node_id] - source_total_lat;
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
                        max_delay,
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

                        let (&output, new_handle_ref) = allocator.find_free_buffer();

                        assert!(dest_node_claims
                            .insert(
                                dest_port_id,
                                (
                                    Rc::clone(new_handle_ref),
                                    InputBufferAssignment {
                                        node: node_id,
                                        port: port_id,
                                        kind: SourceType::Sum {
                                            index: sum_tasks.len(),
                                        },
                                    },
                                ),
                            )
                            .is_none());

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

            insert_new(
                &mut tasks,
                node_id,
                ScheduleEntry {
                    inputs,
                    outputs,
                },
            )
        }

        GraphSchedule {
            num_buffers: allocator.len(),
            tasks,
        }
    }
}
