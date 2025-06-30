use super::*;

/// Inserts a key-value pair into a map
///
/// # Panics
///
/// if `map.contains_key(&k)`
fn insert_new<K: Hash + Eq, V>(map: &mut HashMap<K, V>, k: K, v: V) {
    assert!(map.insert(k, v).is_none());
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum SourceType {
    Direct { delay: u64 },
    Sum { index: usize },
}

impl SourceType {
    #[inline]
    #[must_use]
    pub fn delay(&self) -> u64 {
        match &self {
            SourceType::Direct { delay } => *delay,
            SourceType::Sum { .. } => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Source<N, O> {
    pub node: N,
    pub port: O,
    pub kind: SourceType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SumTask<N, O> {
    pub rhs_delay: u64,
    pub lhs: Source<N, O>,
    pub output: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sink<N, O> {
    pub buf_id: u32,
    pub max_delay: u64,
    pub sum_tasks: Box<[SumTask<N, O>]>,
}

#[derive(Clone, Debug)]
pub struct NodeIO<N, I, O> {
    pub inputs: HashMap<I, Source<N, O>>,
    pub outputs: HashMap<O, Sink<N, O>>,
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> PartialEq for NodeIO<N, I, O> {
    fn eq(&self, other: &Self) -> bool {
        self.inputs == other.inputs && self.outputs == other.outputs
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> Eq for NodeIO<N, I, O> {}

impl<N, I, O> Default for NodeIO<N, I, O> {
    fn default() -> Self {
        Self {
            inputs: HashMap::default(),
            outputs: HashMap::default(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct BufferAllocator {
    ids: Vec<Rc<()>>,
}

impl BufferAllocator {
    #[inline]
    pub(crate) fn len(&self) -> u32 {
        self.ids
            .len()
            .try_into()
            .expect("more than u32::MAX buffers, aborting")
    }

    #[inline]
    fn find_free_buffer(&mut self) -> (u32, &Rc<()>) {
        let id = self
            .ids
            .iter()
            .enumerate()
            .find(|(_, claims)| Rc::strong_count(claims) == 1)
            .map(|(id, _)| id.try_into().unwrap())
            .unwrap_or_else(|| {
                let new_id = self.len();
                self.ids.push(Rc::new(()));
                new_id
            });

        (id, &self.ids[id as usize])
    }
}

#[derive(Debug, Clone)]
pub struct Scheduler<'a, N, I, O> {
    graph: &'a Graph<N, I, O>,
    order: Vec<N>,
    intermediate: HashMap<N, IScheduleEntry<N, I, O>>,
}

impl<N, I, O> Scheduler<'_, N, I, O> {
    #[must_use]
    pub fn intermediate(&self) -> &HashMap<N, IScheduleEntry<N, I, O>> {
        &self.intermediate
    }

    #[must_use]
    pub fn order(&self) -> &[N] {
        &self.order
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Task<N, O> {
    Sum {
        node_id: N,
        port_id: O,
        index: usize,
    },
    Node(N),
}

impl<N: fmt::Debug, O: fmt::Debug> fmt::Debug for Task<N, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sum {
                node_id,
                port_id,
                index,
            } => write!(f, "Sum({node_id:?}, {port_id:?}, {index:?})"),
            Self::Node(id) => write!(f, "Node({id:?})"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IScheduleEntry<N, I, O> {
    pub max_delay: u64,
    pub outputs: HashMap<O, Port<N, I>>,
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> PartialEq for IScheduleEntry<N, I, O> {
    fn eq(&self, other: &Self) -> bool {
        self.max_delay == other.max_delay && self.outputs == other.outputs
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> Eq for IScheduleEntry<N, I, O> {}

#[derive(Debug, Clone)]
pub struct GraphSchedule<N, I, O> {
    pub num_buffers: u32,
    pub node_io: HashMap<N, NodeIO<N, I, O>>,
    pub tasks: Vec<Task<N, O>>,
}

impl<N, I, O> Default for GraphSchedule<N, I, O> {
    fn default() -> Self {
        Self {
            num_buffers: 0,
            node_io: HashMap::default(),
            tasks: Vec::default(),
        }
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> PartialEq for GraphSchedule<N, I, O> {
    fn eq(&self, other: &Self) -> bool {
        self.num_buffers == other.num_buffers
            && self.node_io == other.node_io
            && self.tasks == other.tasks
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> Eq for GraphSchedule<N, I, O> {}

impl<'a, N, I, O> Scheduler<'a, N, I, O> {
    #[inline]
    pub(crate) fn for_graph(graph: &'a Graph<N, I, O>) -> Self {
        Self {
            graph,
            intermediate: HashMap::default(),
            order: Vec::new(),
        }
    }
}

impl<N, I, O> Scheduler<'_, N, I, O>
where
    N: Hash + Eq + Clone,
    I: Hash + Eq + Clone,
    O: Hash + Eq + Clone,
{
    pub fn add_sink_node(&mut self, index: N) {
        if self.intermediate.contains_key(&index) {
            return;
        }

        let mut max_input_lat = 0;

        for (dest_port_id, dest_port) in self.graph.get_node(&index).unwrap().input_ports() {
            for (source_node_id, source_port_ids) in dest_port.connections() {
                self.add_sink_node(source_node_id.clone());

                let IScheduleEntry {
                    max_delay: source_node_input_lat,
                    outputs: source_node_outputs,
                } = self.intermediate.get_mut(source_node_id).unwrap();

                for source_port_id in source_port_ids {
                    source_node_outputs
                        .entry(source_port_id.clone())
                        .or_default()
                        .insert_connection(index.clone(), dest_port_id.clone());

                    let source_port_lat = self
                        .graph
                        .get_node(source_node_id)
                        .unwrap()
                        .output_latencies()[source_port_id];
                    let source_port_total_lat = *source_node_input_lat + source_port_lat;

                    max_input_lat = max_input_lat.max(source_port_total_lat);
                }
            }
        }

        self.order.push(index.clone());

        assert!(
            self.intermediate
                .insert(
                    index,
                    IScheduleEntry {
                        max_delay: max_input_lat,
                        outputs: HashMap::default()
                    }
                )
                .is_none()
        );
    }

    pub fn compile(&self) -> GraphSchedule<N, I, O> {
        let mut allocator = BufferAllocator::default();

        let mut claims = HashMap::<N, HashMap<I, (Rc<()>, Source<N, O>)>>::default();

        let mut node_io = HashMap::<N, NodeIO<N, I, O>>::default();

        let mut tasks = vec![];

        let Self {
            graph,
            intermediate,
            order,
        } = self;

        for node_id in order {
            let IScheduleEntry {
                max_delay: max_input_lat,
                outputs: node_outputs,
            } = &intermediate[node_id];

            tasks.push(Task::Node(node_id.clone()));

            let node_output_lats = graph.get_node(node_id).unwrap().output_latencies();

            let mut inputs = HashMap::default();
            let mut outputs = HashMap::default();

            let mut repeat_assignees = HashMap::default();

            // for every (actually used) output of this node

            for (source_port_id, source_port) in node_outputs {
                let connections = source_port.connections();
                if connections.is_empty() {
                    continue;
                }

                // allocate a buffer for it
                let (buf_id, handle_ref) = allocator.find_free_buffer();

                let source_total_lat = max_input_lat + node_output_lats[source_port_id];
                let mut max_delay = 0;

                let mut repeats = HashMap::default();

                for (dest_node_id, dest_port_ids) in connections {
                    let mut prev_assigned_ports = HashMap::default();

                    // find the maximum delay it will be subjected to
                    let delay = intermediate[dest_node_id].max_delay - source_total_lat;
                    max_delay = max_delay.max(delay);

                    // assign the buffer to all recieveing ports, and keep track of ports
                    // that have already been assigned a buffer
                    for dest_port_id in dest_port_ids {
                        let handle = Rc::clone(handle_ref);

                        if claims
                            .get(dest_node_id)
                            .is_some_and(|map| map.contains_key(dest_port_id))
                        {
                            prev_assigned_ports.insert(dest_port_id, (handle, delay));
                        } else {
                            claims.entry(dest_node_id.clone()).or_default().insert(
                                dest_port_id.clone(),
                                (
                                    handle,
                                    Source {
                                        node: node_id.clone(),
                                        port: source_port_id.clone(),
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
                    source_port_id.clone(),
                    Sink {
                        buf_id,
                        max_delay,
                        sum_tasks: Box::new([]),
                    },
                );
            }

            // handle repeat assignments
            for (port_id, repeat_assignees) in repeat_assignees {
                let mut sum_tasks = vec![];

                for (dest_node_id, repeat_assignees) in repeat_assignees {
                    let dest_node_claims = claims.get_mut(dest_node_id).unwrap();

                    for (dest_port_id, (other_old_handle, delay)) in repeat_assignees {
                        // we're not using insert directly to allow dropping
                        // {this, other}_old_handle early

                        let (this_old_handle, lhs) = dest_node_claims.remove(dest_port_id).unwrap();

                        // because we can potentially reuse the input buffers if they have no latency
                        if delay == 0 {
                            drop(other_old_handle);
                        }

                        if lhs.kind.delay() == 0 {
                            drop(this_old_handle);
                        }

                        let (output, new_handle_ref) = allocator.find_free_buffer();

                        let index = sum_tasks.len();

                        assert!(
                            dest_node_claims
                                .insert(
                                    dest_port_id.clone(),
                                    (
                                        Rc::clone(new_handle_ref),
                                        Source {
                                            node: node_id.clone(),
                                            port: port_id.clone(),
                                            kind: SourceType::Sum { index },
                                        },
                                    ),
                                )
                                .is_none()
                        );

                        tasks.push(Task::Sum {
                            node_id: node_id.clone(),
                            port_id: port_id.clone(),
                            index,
                        });

                        sum_tasks.push(SumTask {
                            rhs_delay: delay,
                            lhs,
                            output,
                        });
                    }
                }

                outputs.get_mut(port_id).unwrap().sum_tasks = sum_tasks.into_boxed_slice();
            }

            for (dest_port_id, (_handle, source)) in claims
                .get_mut(node_id)
                .into_iter()
                .flat_map(HashMap::drain)
            {
                insert_new(&mut inputs, dest_port_id.clone(), source);
            }

            assert!(
                node_io.insert(node_id.clone(), NodeIO { inputs, outputs })
                    .is_none()
            );
        }

        GraphSchedule {
            num_buffers: allocator.len(),
            node_io,
            tasks,
        }
    }
}
