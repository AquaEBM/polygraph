use crate::errors::{CycleFound, EdgeNotFound};

use super::*;

use core::ops::{Index, IndexMut};

fn insert_at_next_empty_slot<T>(vec: &mut StableVec<T>, item: T) -> usize {
    if let Some(i) = vec.first_empty_slot_from(0) {
        vec.insert(i, item);
        i
    } else {
        vec.push(item)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Ports(HashMap<NodeIndex, HashSet<usize>>);

impl Ports {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter_nodes(&self) -> impl Iterator<Item = &NodeIndex> {
        self.0.keys()
    }

    pub fn iter_ports<'a>(&'a self) -> impl Iterator<Item = Port> + 'a {
        self.0
            .iter()
            .map(|(&node_index, port_idxs)| {
                port_idxs
                    .iter()
                    .map(move |&index| Port { index, node_index })
            })
            .flatten()
    }

    pub(super) fn insert_port(&mut self, Port { index, node_index }: Port) -> bool {
        self.0
            .entry(node_index)
            .or_insert_with(HashSet::default)
            .insert(index)
    }

    pub fn remove_port(&mut self, Port { index, node_index }: &Port) -> bool {
        if let Some(port_idxs) = self.0.get_mut(node_index) {
            // (0w0) Oooh? Since when was the borrow checker this smart?
            if port_idxs.len() == 1 {
                self.0.remove(node_index);
                true
            } else {
                port_idxs.remove(index)
            }
        } else {
            false
        }
    }

    pub fn remove_all_ports_to_node(&mut self, node_index: &NodeIndex) -> Option<HashSet<usize>> {
        self.0.remove(node_index)
    }
}

#[derive(Debug, Clone)]
pub struct NodeIO {
    ports: Box<[Ports]>,
    num_opposite_ports: usize,
}

impl NodeIO {
    pub(super) fn with_io_config(num_ports: usize, num_opposite_ports: usize) -> Self {
        Self {
            ports: iter::repeat_with(Ports::default).take(num_ports).collect(),
            num_opposite_ports,
        }
    }

    pub(super) fn with_opposite_config(&self) -> Self {
        Self::with_io_config(self.num_opposite_ports(), self.ports().len())
    }

    pub(super) fn num_opposite_ports(&self) -> usize {
        self.num_opposite_ports
    }

    pub fn num_outputs(&self) -> usize {
        self.num_opposite_ports()
    }

    pub(super) fn ports(&self) -> &[Ports] {
        self.ports.as_ref()
    }

    pub fn inputs(&self) -> &[Ports] {
        self.ports()
    }

    pub(super) fn ports_mut(&mut self) -> &mut [Ports] {
        self.ports.as_mut()
    }

    pub fn inputs_mut(&mut self) -> &mut [Ports] {
        self.ports_mut()
    }

    pub(super) fn get_connections(&self, index: usize) -> Option<&Ports> {
        self.ports().get(index)
    }

    pub(super) fn get_connections_mut(&mut self, index: usize) -> Option<&mut Ports> {
        self.ports_mut().get_mut(index)
    }
}

#[derive(Debug, Clone)]
pub(super) struct AudioGraphIO {
    processors: StableVec<NodeIO>,
    global: NodeIO,
}

impl AudioGraphIO {
    pub(super) fn with_global_io_config(
        num_global_io_ports: usize,
        num_opposite_global_io_ports: usize,
    ) -> Self {
        Self {
            processors: StableVec::default(),
            global: NodeIO::with_io_config(num_opposite_global_io_ports, num_global_io_ports),
        }
    }

    pub(super) fn with_opposite_config(&self) -> Self {
        let mut processors = StableVec::with_capacity(self.processors.capacity());
        self.processors.iter().for_each(|(i, interface)| {
            processors.insert(i, interface.with_opposite_config());
        });

        Self {
            global: self.global.with_opposite_config(),
            processors,
        }
    }

    pub(super) fn iter_processor_io(&self) -> impl Iterator<Item = (usize, &NodeIO)> {
        self.processors.iter()
    }

    pub(super) fn iter_mut_processor_io(&mut self) -> impl Iterator<Item = (usize, &mut NodeIO)> {
        self.processors.iter_mut()
    }

    pub(super) fn get_node(&self, index: NodeIndex) -> Option<&NodeIO> {
        match index {
            NodeIndex::Global => Some(&self.global),
            NodeIndex::Processor(i) => self.processors.get(i),
        }
    }

    pub(super) fn get_node_mut(&mut self, index: NodeIndex) -> Option<&mut NodeIO> {
        match index {
            NodeIndex::Global => Some(&mut self.global),
            NodeIndex::Processor(i) => self.processors.get_mut(i),
        }
    }

    pub(super) fn get_connections(&self, port: Port) -> Option<&Ports> {
        self.get_node(port.node_index)
            .map(|interface| interface.get_connections(port.index))
            .flatten()
    }

    pub(super) fn get_connections_mut(&mut self, port: Port) -> Option<&mut Ports> {
        self.get_node_mut(port.node_index)
            .map(|interface| interface.get_connections_mut(port.index))
            .flatten()
    }

    pub(super) fn connected(
        &self,
        from_node: NodeIndex,
        to_node: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
    ) -> bool {
        if from_node == to_node {
            return true;
        }
        if !visited.insert(from_node) {
            return false;
        }

        self[from_node].ports().iter().any(|ports| {
            ports
                .iter_nodes()
                .any(|&node| self.connected(node, to_node, visited))
        })
    }

    pub(super) fn insert_processor(
        &mut self,
        num_ports: usize,
        num_opposite_ports: usize,
    ) -> usize {
        insert_at_next_empty_slot(
            &mut self.processors,
            NodeIO::with_io_config(num_ports, num_opposite_ports),
        )
    }

    pub(super) fn remove_processor(&mut self, index: usize) -> bool {
        self.processors
            .remove(index)
            .map(|_proc| {
                for interface in self.processors.values_mut() {
                    for ports in interface.ports_mut() {
                        ports.remove_all_ports_to_node(&NodeIndex::Processor(index));
                    }
                }
            })
            .is_some()
    }

    pub(super) fn remove_edge(&mut self, from: Port, to: Port) -> Result<bool, EdgeNotFound> {
        let error = EdgeNotFound {
            from_port: self
                .get_node(from.node_index)
                .map(|interface| interface.get_connections(from.index).is_some()),
            to_port: self
                .get_node(to.node_index)
                .map(|interface| to.index < interface.num_opposite_ports()),
        };

        if error.is_not_error() {
            Ok(self.get_connections_mut(from).unwrap().remove_port(&to))
        } else {
            Err(error)
        }
    }

    pub(super) fn opposite_port_indices(
        &self,
        node_index: NodeIndex,
    ) -> impl Iterator<Item = Port> {
        (0..self[node_index].num_opposite_ports()).map(move |index| Port { index, node_index })
    }

    pub(super) fn insert_opposite_ports(
        &mut self,
        inputs: &AudioGraphIO,
        node_index: NodeIndex,
        registered: &mut HashSet<NodeIndex>,
        register_order: &mut Vec<NodeIndex>,
    ) {
        for (i, incoming_ports) in inputs[node_index].ports().iter().enumerate() {
            let this_port = Port::new(i, node_index);
            for port in incoming_ports.iter_ports() {
                self[port].insert_port(this_port);

                let next_idx = port.node_index;

                if !registered.contains(&next_idx) {
                    if !next_idx.is_global() {
                        self.insert_opposite_ports(inputs, next_idx, registered, register_order)
                    }

                    registered.insert(next_idx);
                    register_order.push(next_idx);
                }
            }
        }
    }

    pub(super) fn insert_edge(&mut self, from: Port, to: Port) -> Result<bool, EdgeInsertError> {
        let error = EdgeNotFound {
            from_port: self
                .get_node(from.node_index)
                .map(|interface| interface.get_connections(from.index).is_some()),
            to_port: self
                .get_node(to.node_index)
                .map(|interface| to.index < interface.num_opposite_ports()),
        };

        if error.is_not_error() {
            // global "nodes" have either only inputs or only outputs. It's thus
            // not possible to create a cycle by inserting an edge with a global
            // node in either of it's extremities
            if !(from.node_index.is_global() || to.node_index.is_global()) {
                let mut visited = HashSet::default();

                // cycle detected
                if self.connected(to.node_index, from.node_index, &mut visited) {
                    return Err(EdgeInsertError::CycleFound(CycleFound));
                }
            }

            Ok(self[from].insert_port(to))
        } else {
            Err(EdgeInsertError::NotFound(error))
        }
    }
}

impl Index<NodeIndex> for AudioGraphIO {
    type Output = NodeIO;

    fn index(&self, index: NodeIndex) -> &Self::Output {
        self.get_node(index).unwrap()
    }
}

impl IndexMut<NodeIndex> for AudioGraphIO {
    fn index_mut(&mut self, index: NodeIndex) -> &mut Self::Output {
        self.get_node_mut(index).unwrap()
    }
}

impl Index<Port> for AudioGraphIO {
    type Output = Ports;

    fn index(&self, port: Port) -> &Self::Output {
        self.get_connections(port).unwrap()
    }
}

impl IndexMut<Port> for AudioGraphIO {
    fn index_mut(&mut self, port: Port) -> &mut Self::Output {
        self.get_connections_mut(port).unwrap()
    }
}
