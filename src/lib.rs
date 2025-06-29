use core::{borrow, fmt, hash::Hash};

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

#[derive(Clone, Debug)]
pub struct Graph<N, I, O> {
    nodes: HashMap<N, Node<N, I, O>>,
}

impl<N, I, O> Default for Graph<N, I, O> {
    fn default() -> Self {
        Self {
            nodes: HashMap::default(),
        }
    }
}

impl<N, I, O> PartialEq for Graph<N, I, O>
where
    N: Hash + Eq,
    I: Hash + Eq,
    O: Hash + Eq,
{
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
    }
}

impl<N, I, O> Eq for Graph<N, I, O>
where
    N: Hash + Eq,
    I: Hash + Eq,
    O: Hash + Eq,
{
}

impl<N: Hash + Eq, I, O> Graph<N, I, O> {
    #[inline]
    pub fn try_insert_edge<Q, R>(&mut self, from: (N, O), to: (&Q, &R)) -> Result<bool, bool>
    where
        N: borrow::Borrow<Q>,
        I: Hash + Eq + borrow::Borrow<R>,
        O: Hash + Eq,
        Q: ?Sized + Hash + Eq,
        R: ?Sized + Hash + Eq,
    {
        let (source_node, source_output) = from;
        let (dest_node, dest_input) = to;

        let source_node_borrowed = borrow::Borrow::<Q>::borrow(&source_node);

        // If either of the ports don't exist, error out
        if self
            .get_node(dest_node)
            .is_none_or(|node| !node.input_ports().contains_key(dest_input))
            || self
                .get_node(source_node_borrowed)
                .is_none_or(|node| !node.output_latencies().contains_key(&source_output))
        {
            return Err(false);
        }

        if self.is_connected(source_node_borrowed, dest_node) {
            return Err(true);
        }

        Ok(self
            .get_node_mut(dest_node)
            .unwrap()
            .get_port_mut(dest_input)
            .unwrap()
            .insert_connection(source_node, source_output))
    }

    /// # Panics
    ///
    /// if no node with ids `from` or `to` exist
    fn is_connected<Q>(&self, from: &Q, to: &Q) -> bool
    where 
        N: borrow::Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if from == to {
            return true;
        }

        for port in self.get_node(from).unwrap().input_ports().values() {
            for node in port.connections().keys() {
                if self.is_connected(node.borrow(), to) {
                    return true;
                }
            }
        }

        false
    }

    #[inline]
    #[must_use]
    pub fn get_node<Q>(&self, id: &Q) -> Option<&Node<N, I, O>>
    where 
        N: borrow::Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.nodes.get(id)
    }

    #[inline]
    pub fn get_node_mut<Q>(&mut self, id: &Q) -> Option<&mut Node<N, I, O>>
    where 
        N: borrow::Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.nodes.get_mut(id)
    }

    #[inline]
    pub fn insert_node(&mut self, id: N, node: Node<N, I, O>) {
        self.nodes.insert(id, node);
    }

    #[inline]
    #[must_use]
    pub fn scheduler(&self) -> Scheduler<'_, N, I, O> {
        Scheduler::for_graph(self)
    }
}
