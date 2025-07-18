use super::*;

/// Inserts a key-value pair into a map
///
/// # Panics
///
/// if `map.contains_key(&k)`
#[inline]
fn insert_new<K: Hash + Eq, V>(map: &mut HashMap<K, V>, k: K, v: V) {
    assert!(map.insert(k, v).is_none());
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputSource<N, O> {
    GraphNode { delay: u64, node_id: N, port_id: O },
    SumNode { index: usize },
}

impl<N, O> InputSource<N, O> {
    #[inline]
    #[must_use]
    pub fn delay(&self) -> u64 {
        match self {
            InputSource::GraphNode { delay, .. } => *delay,
            InputSource::SumNode { .. } => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeOutput<N, O, T = u64> {
    pub buf_id: u32,
    pub max_delay: T,
    pub connections: Port<N, O>,
}

impl<N: Hash + Eq, O: Hash + Eq, T: PartialEq> PartialEq for NodeOutput<N, O, T> {
    fn eq(&self, other: &Self) -> bool {
        self.buf_id == other.buf_id
            && self.max_delay == other.max_delay
            && self.connections == other.connections
    }
}

impl<N: Hash + Eq, O: Hash + Eq, T: Eq> Eq for NodeOutput<N, O, T> {}

#[derive(Clone, Debug)]
pub struct NodeIO<N, I, O, T = u64> {
    pub max_delay: u64,
    pub inputs: HashMap<I, Option<InputSource<N, O>>>,
    pub outputs: HashMap<O, Option<NodeOutput<N, O, T>>>,
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq, T: PartialEq> PartialEq for NodeIO<N, I, O, T> {
    fn eq(&self, other: &Self) -> bool {
        self.max_delay == other.max_delay && self.inputs == other.inputs && self.outputs == other.outputs
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq, T: Eq> Eq for NodeIO<N, I, O, T> {}

impl<N, I, O, T> Default for NodeIO<N, I, O, T> {
    fn default() -> Self {
        Self {
            max_delay: 0,
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
        self.ids.len().try_into().unwrap()
    }

    #[inline]
    fn find_free_buffer(&mut self) -> (u32, &Rc<()>) {
        let len = self.len();
        let id = self
            .ids
            .iter()
            .zip(0u32..)
            .find(|(claims, _)| Rc::strong_count(claims) == 1)
            .map_or(len, |(_, id)| id);

        if id == len {
            self.ids.push(Rc::new(()));
        }

        (id, &self.ids[id as usize])
    }
}

#[derive(Clone, Debug)]
pub struct UsedNode<N, I, O> {
    pub max_delay: u64,
    pub used_outputs: HashMap<O, Port<N, I>>,
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> PartialEq for UsedNode<N, I, O> {
    fn eq(&self, other: &Self) -> bool {
        self.max_delay == other.max_delay && self.used_outputs == other.used_outputs
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq> Eq for UsedNode<N, I, O> {}

#[derive(Debug, Clone)]
pub struct Scheduler<'a, N, I, O, T = u64> {
    graph: &'a Graph<N, I, O>,
    order: Vec<N>,
    node_io: HashMap<N, NodeIO<N, I, O, T>>,
}

impl<N, I, O, T> Scheduler<'_, N, I, O, T> {

    #[must_use]
    pub fn order(&self) -> &[N] {
        self.order.as_slice()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Task<N> {
    Sum(usize),
    Node(N),
}

impl<N: fmt::Debug> fmt::Debug for Task<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sum(i) => write!(f, "Sum({i:?})"),
            Self::Node(n) => write!(f, "Node({n:?})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SumNode<N, O> {
    pub summands: [InputSource<N, O>; 2],
    pub output_buf: u32,
}

#[derive(Debug, Clone)]
pub struct GraphSchedule<N, I, O, T = u64> {
    pub num_buffers: u32,
    pub node_io: HashMap<N, NodeIO<N, I, O, T>>,
    pub sum_nodes: Vec<SumNode<N, O>>,
    pub tasks: Vec<Task<N>>,
}

impl<N, I, O, T> Default for GraphSchedule<N, I, O, T> {
    fn default() -> Self {
        Self {
            num_buffers: 0,
            node_io: HashMap::default(),
            sum_nodes: Vec::default(),
            tasks: Vec::default(),
        }
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq, T: PartialEq> PartialEq
    for GraphSchedule<N, I, O, T>
{
    fn eq(&self, other: &Self) -> bool {
        self.num_buffers == other.num_buffers
            && self.node_io == other.node_io
            && self.tasks == other.tasks
    }
}

impl<N: Hash + Eq, I: Hash + Eq, O: Hash + Eq, T: Eq> Eq for GraphSchedule<N, I, O, T> {}

impl<'a, N, I, O, T> Scheduler<'a, N, I, O, T> {
    #[inline]
    pub(crate) fn for_graph(graph: &'a Graph<N, I, O>) -> Self {
        Self {
            graph,
            node_io: HashMap::default(),
            order: Vec::new(),
        }
    }
}

impl<N, I, O, T> Scheduler<'_, N, I, O, T>
where
    N: Hash + Eq + Clone,
    I: Hash + Eq + Clone,
    O: Hash + Eq + Clone,
{
    pub fn add_sink_node(&mut self, index: N) {
        if self.node_io.contains_key(&index) {
            return;
        }

        let mut max_input_lat = 0;

        for (dest_port_id, dest_port) in self.graph.get_node(&index).unwrap().input_ports() {
            for (source_node_id, source_port_ids) in dest_port.connections() {
                self.add_sink_node(source_node_id.clone());

                let NodeIO {
                    max_delay: source_node_input_lat,
                    outputs: source_node_outputs,
                } = self.node_io.get_mut(source_node_id).unwrap();

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
                    UsedNode {
                        max_delay: max_input_lat,
                        used_outputs: HashMap::default()
                    }
                )
                .is_none()
        );
    }

    pub fn compile_map_delays<T>(&self, f: impl Fn(u64) -> T) -> GraphSchedule<N, I, O, T> {
        let mut allocator = BufferAllocator::default();

        let mut claims = HashMap::<N, HashMap<I, (Rc<()>, InputSource<N, O>)>>::default();

        let mut node_io = HashMap::<N, NodeIO<N, I, O, T>>::default();

        let mut sum_nodes = Vec::default();

        let mut tasks = vec![];

        let Self {
            graph,
            intermediate,
            order,
        } = self;

        for node_id in order {
            let UsedNode {
                max_delay,
                used_outputs,
            } = &intermediate[node_id];

            tasks.push(Task::Node(node_id.clone()));

            let graph_node = graph.get_node(node_id).unwrap();
            let node_output_lats = graph_node.output_latencies();
            let node_inputs = graph_node.input_ports();

            let mut inputs = HashMap::default();
            let mut outputs = HashMap::default();

            let mut repeat_assignees = HashMap::default();

            // for every (actually used) output of this node

            for (source_port_id, output_lat) in node_output_lats {
                let Some(source_port) = used_outputs.get(source_port_id) else {
                    outputs.insert(source_port_id.clone(), None);
                    continue;
                };

                // this is never empty
                let connections = source_port.connections();
                assert!(!connections.is_empty());

                // allocate a buffer for it
                let (buf_id, handle_ref) = allocator.find_free_buffer();

                let source_total_lat = max_delay + output_lat;
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
                                    InputSource::GraphNode {
                                        node_id: node_id.clone(),
                                        port_id: source_port_id.clone(),
                                        delay,
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
                    Some(NodeOutput {
                        buf_id,
                        max_delay: f(max_delay),
                    }),
                );
            }

            // handle repeat assignments
            for (source_port_id, repeat_assignees) in repeat_assignees {
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

                        if lhs.delay() == 0 {
                            drop(this_old_handle);
                        }

                        let (output_buf, new_handle_ref) = allocator.find_free_buffer();

                        let index = sum_nodes.len();

                        assert!(
                            dest_node_claims
                                .insert(
                                    dest_port_id.clone(),
                                    (Rc::clone(new_handle_ref), InputSource::SumNode { index }),
                                )
                                .is_none()
                        );

                        tasks.push(Task::Sum(index));

                        sum_nodes.push(SumNode {
                            summands: [
                                lhs,
                                InputSource::GraphNode {
                                    node_id: node_id.clone(),
                                    port_id: source_port_id.clone(),
                                    delay,
                                },
                            ],
                            output_buf,
                        });
                    }
                }
            }

            if let Some(claimed) = claims.get_mut(node_id) {
                for dest_port_id in node_inputs.keys() {
                    let source = claimed.remove(dest_port_id).map(|(_handle, source)| source);
                    insert_new(&mut inputs, dest_port_id.clone(), source);
                }
            }

            assert!(
                node_io
                    .insert(node_id.clone(), NodeIO { inputs, outputs })
                    .is_none()
            );
        }

        GraphSchedule {
            num_buffers: allocator.len(),
            node_io,
            sum_nodes,
            tasks,
        }
    }

    pub fn compile(&self) -> GraphSchedule<N, I, O> {
        self.compile_map_delays(|x| x)
    }
}
