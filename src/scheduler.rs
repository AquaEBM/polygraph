use super::*;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct BufferID(NonZeroU32);

impl Debug for BufferID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BufferID({:?})", &self.0)
    }
}

impl BufferID {
    #[inline]
    pub(crate) fn new_key<H: BuildHasher, V>(map: &HashMap<Self, V, H>) -> Self {
        let mut id = Self(NonZeroU32::MIN);

        while map.contains_key(&id) {
            id.0 = id.0.checked_add(1).expect("Index overflow");
        }

        id
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct DelayBufferAssignment {
    pub delay: u64,
    pub port: (NodeID, OutputID),
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct InputBufferAssignment {
    pub index: BufferID,
    pub delay_id: Option<DelayBufferAssignment>,
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct SumTask {
    pub summand: InputBufferAssignment,
    pub delay: u64,
    pub output: BufferID,
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct OutputBufferAssignment {
    pub index: BufferID,
    pub sum_tasks: Box<[SumTask]>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct NodeProcessTask {
    pub id: NodeID,
    pub inputs: HashMap<InputID, InputBufferAssignment>,
    pub outputs: HashMap<OutputID, OutputBufferAssignment>,
}

impl NodeProcessTask {
    #[inline]
    fn new(id: NodeID) -> Self {
        Self {
            id,
            inputs: Default::default(),
            outputs: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct BufferAllocator {
    pub(crate) buffers: HashMap<(NodeID, InputID), (BufferID, Option<(NodeID, OutputID)>)>,
    pub(crate) ports: HashMap<BufferID, HashSet<(NodeID, InputID)>>,
}

impl BufferAllocator {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.ports.len()
    }

    #[inline]
    fn find_free_buffer(&mut self) -> BufferID {
        self.ports
            .iter()
            .find(|(_, ports)| ports.is_empty())
            .map(|(&id, _)| id)
            .unwrap_or_else(|| {
                let new_key = BufferID::new_key(&self.ports);
                assert!(self.ports.insert(new_key, HashSet::default()).is_none());
                new_key
            })
    }

    pub(crate) fn claim_free<T: IntoIterator<Item = (NodeID, InputID)>>(
        &mut self,
        source: Option<(NodeID, OutputID)>,
        dests: T,
    ) -> (
        BufferID,
        impl Iterator<
            Item = (
                (BufferID, (NodeID, InputID)),
                (BufferID, Option<(NodeID, OutputID)>),
            )
        > + use<'_, T>,
    ) {
        let new_buf = self.find_free_buffer();
        (
            new_buf,
            dests.into_iter().filter_map(move |dest_port| {
                if let Some(source) = self.remove_claim(&dest_port) {
                    let (dest_buf, mut preex_claims) = self.claim_free(None, [dest_port]);
    
                    assert!(preex_claims.next().is_none());
                    Some(((dest_buf, dest_port), source))
                } else {
                    self.ports.get_mut(&new_buf).unwrap().insert(dest_port);
                    assert!(self.buffers.insert(dest_port, (new_buf, source)).is_none());
                    None
                }
            }),    
        )
    }

    #[inline]
    fn remove_claim(
        &mut self,
        port: &(NodeID, InputID),
    ) -> Option<(BufferID, Option<(NodeID, OutputID)>)> {
        self.buffers
            .remove(port)
            .inspect(|(buf, _)| assert!(self.ports.get_mut(buf).unwrap().remove(port)))
    }
}

#[derive(Debug, Clone)]
pub struct Scheduler<'a> {
    pub(crate) graph: &'a Graph,
    pub(crate) intermediate: HashMap<NodeID, HashMap<OutputID, Port<InputID>>>,
    pub(crate) schedule: Vec<NodeProcessTask>,
    pub(crate) max_input_lats: HashMap<NodeID, u64>,
    pub(crate) output_max_delays: HashMap<NodeID, HashMap<OutputID, u64>>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct GraphSchedule {
    pub num_buffers: usize,
    pub tasks: Box<[NodeProcessTask]>,
    pub max_input_lats: HashMap<NodeID, u64>,
    pub output_max_delays: HashMap<NodeID, HashMap<OutputID, u64>>,
}

impl<'a> Scheduler<'a> {
    #[inline]
    pub(crate) fn for_graph(graph: &'a Graph) -> Self {
        Self {
            graph,
            intermediate: Default::default(),
            schedule: Default::default(),
            max_input_lats: Default::default(),
            output_max_delays: Default::default(),
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
        self.schedule.push(NodeProcessTask::new(*index));
    }

    pub fn compile(mut self) -> GraphSchedule {
        let mut allocator = BufferAllocator::default();

        for NodeProcessTask {
            ref id,
            inputs,
            outputs,
        } in &mut self.schedule
        {
            let current_node = &self.graph[id];
            let node_output_lats = current_node.output_latencies();
            let max_input_lat = self.max_input_lats[id];

            let node_outputs = &self.intermediate[id];

            let mut output_max_delays = HashMap::default();

            // for every (actually used) output of this node

            for (&source_id, source_port) in node_outputs.iter() {
                let connections = source_port.connections();
                if connections.is_empty() {
                    continue;
                }

                let source_total_lat = max_input_lat + node_output_lats[&source_id];

                // find the maximum delay it will be subjected to

                output_max_delays.insert(
                    source_id,
                    connections
                        .keys()
                        .map(|dest_node_id| self.max_input_lats[dest_node_id] - source_total_lat)
                        .max()
                        .unwrap(),
                );

                // allocate a buffer for it

                let (source_buf, preex_claims) = allocator.claim_free(
                    Some((*id, source_id)),
                    connections
                        .iter()
                        .flat_map(|(&node, ports)| ports.iter().map(move |&p| (node, p))),
                );

                // insert sum tasks for ports that have other incoming outputs
                let sum_tasks = preex_claims
                    .map(|(dest, prev_source)| {
                        let (dest_buf, (dest_node, _)) = dest;

                        let (prev_source_buf, prev_source_port) = prev_source;

                        let dest_total_input_lat = self.max_input_lats[&dest_node];

                        let source_delay = dest_total_input_lat - source_total_lat;

                        SumTask {
                            summand: InputBufferAssignment {
                                index: prev_source_buf,
                                delay_id: prev_source_port.map(|port @ (node, output)| {
                                    DelayBufferAssignment {
                                        delay: dest_total_input_lat
                                            - (self.max_input_lats[&node]
                                                + self.graph[&node].output_latencies()[&output]),
                                        port,
                                    }
                                }),
                            },
                            delay: source_delay,
                            output: dest_buf,
                        }
                    })
                    .collect();

                outputs.insert(
                    source_id,
                    OutputBufferAssignment {
                        index: source_buf,
                        sum_tasks,
                    },
                );
            }
            
            assert!(self.output_max_delays.insert(*id, output_max_delays).is_none());

            for &dest_port_id in current_node.input_ports().keys() {
                if let Some((dest_buf, source)) = allocator.remove_claim(&(*id, dest_port_id)) {
                    inputs.insert(
                        dest_port_id,
                        InputBufferAssignment {
                            index: dest_buf,
                            delay_id: source.map(|port @ (node, output)| DelayBufferAssignment {
                                delay: max_input_lat
                                    - (self.max_input_lats[&node]
                                        + self.graph[&node].output_latencies()[&output]),
                                port,
                            }),
                        },
                    );
                }
            }
        }

        GraphSchedule {
            num_buffers: allocator.len(),
            tasks: self.schedule.into_boxed_slice(),
            max_input_lats: self.max_input_lats,
            output_max_delays: self.output_max_delays,
        }
    }
}
