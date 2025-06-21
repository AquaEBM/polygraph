use core::{
    convert,
    fmt::{self, Debug},
    hash::Hash,
    num::NonZeroU32,
    ops::{Index, IndexMut},
};

extern crate alloc;
use alloc::rc::Rc;

mod port;
pub use port::*;

mod node;
pub use node::*;

pub mod scheduler;
pub use scheduler::*;

#[cfg(test)]
mod tests;

pub type HashMap<K, V> = std::collections::HashMap<K, V, fnv::FnvBuildHasher>;
pub type HashSet<T> = std::collections::HashSet<T, fnv::FnvBuildHasher>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    nodes: HashMap<NodeID, Node>,
}

impl Index<&NodeID> for Graph {
    type Output = Node;
    #[inline]
    fn index(&self, key: &NodeID) -> &Self::Output {
        self.get_node(key).expect("no node found for this id")
    }
}

impl IndexMut<&NodeID> for Graph {
    fn index_mut(&mut self, key: &NodeID) -> &mut Self::Output {
        self.get_node_mut(key).expect("no node found for this id")
    }
}

impl Graph {
    #[inline]
    pub fn try_insert_edge(
        &mut self,
        from: (NodeID, OutputID),
        to: (&NodeID, &InputID),
    ) -> Result<bool, bool> {
        let (source_node, source_output) = from;
        let (dest_node, dest_input) = to;

        // If either of the ports don't exist, error out
        if self
            .get_node(dest_node)
            .is_none_or(|node| !node.input_ports().contains_key(dest_input))
            || self
                .get_node(&source_node)
                .is_none_or(|node| !node.output_latencies().contains_key(&source_output))
        {
            return Err(false);
        }

        if self.is_connected(&source_node, dest_node) {
            return Err(true);
        }

        Ok(self[dest_node][dest_input].insert_connection(source_node, source_output))
    }

    /// # Panics
    ///
    /// if no node with ids `from` or `to` exist
    fn is_connected(&self, from: &NodeID, to: &NodeID) -> bool {
        if from == to {
            return true;
        }

        for port in self[from].input_ports().values() {
            for node in port.connections().keys() {
                if self.is_connected(node, to) {
                    return true;
                }
            }
        }

        false
    }

    #[inline]
    #[must_use]
    pub fn get_node(&self, index: &NodeID) -> Option<&Node> {
        self.nodes.get(index)
    }

    #[inline]
    pub fn get_node_mut(&mut self, index: &NodeID) -> Option<&mut Node> {
        self.nodes.get_mut(index)
    }

    #[inline]
    pub fn insert_node(&mut self) -> (NodeID, &mut Node) {
        let k = NodeID::new_key(&self.nodes);

        self.nodes.insert(k, Node::default());
        // a insert_and_get_mut sort of method would be helpful here
        (k, self.nodes.get_mut(&k).unwrap())
    }

    #[inline]
    #[must_use]
    pub fn scheduler(&self) -> Scheduler<'_> {
        Scheduler::for_graph(self)
    }
}
