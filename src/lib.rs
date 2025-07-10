use core::{borrow, fmt, hash::Hash};

extern crate alloc;
use alloc::rc::Rc;

pub mod scheduler;
pub use scheduler::*;

#[cfg(test)]
mod tests;

pub type HashMap<K, V> = std::collections::HashMap<K, V, fnv::FnvBuildHasher>;
pub type HashSet<T> = std::collections::HashSet<T, fnv::FnvBuildHasher>;

#[derive(Clone)]
pub struct Port<N, O>(HashMap<N, HashSet<O>>);

impl<N: fmt::Debug, O: fmt::Debug> fmt::Debug for Port<N, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct IgnoreAlt<A, B>(A, B);

        impl<A: fmt::Debug, B: fmt::Debug> fmt::Debug for IgnoreAlt<A, B> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "(")?;
                fmt::Debug::fmt(&self.0, f)?;
                write!(f, ", ")?;
                fmt::Debug::fmt(&self.1, f)?;
                write!(f, ")")
            }
        }

        let mut debug_list = f.debug_list();

        debug_list.entries(
            self.iter_connections()
                .map(|(node_id, port_id)| IgnoreAlt(node_id, port_id)),
        );

        debug_list.finish()
    }
}

impl<N, O> Default for Port<N, O> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

impl<N: Hash + Eq, O: Hash + Eq> PartialEq for Port<N, O> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<N: Hash + Eq, O: Hash + Eq> Eq for Port<N, O> {}

impl<N, O> Port<N, O> {
    #[inline]
    #[must_use]
    pub fn connections(&self) -> &HashMap<N, HashSet<O>> {
        &self.0
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.values().map(HashSet::len).sum()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn iter_connections(&self) -> impl Iterator<Item = (&N, &O)> {
        self.0
            .iter()
            .flat_map(|(node_id, ports)| ports.iter().map(move |port| (node_id, port)))
    }
}

impl<N: Hash + Eq, O: Hash + Eq> Port<N, O> {
    #[inline]
    pub(crate) fn insert_connection(&mut self, node_index: N, port_index: O) -> bool {
        self.0.entry(node_index).or_default().insert(port_index)
    }

    #[inline]
    pub fn remove_port<Q, R>(&mut self, node_index: &Q, port_index: &R) -> bool
    where
        Q: ?Sized + Hash + Eq,
        R: ?Sized + Hash + Eq,
        N: borrow::Borrow<Q>,
        O: borrow::Borrow<R>,
    {
        let mut empty = false;

        let tmp = self.0.get_mut(node_index).is_some_and(|ports| {
            let tmp = ports.remove(port_index);
            empty = ports.is_empty();
            tmp
        });

        if empty {
            self.0.remove(node_index);
        }

        tmp
    }
}

#[derive(Clone, Debug)]
pub struct Node<N, I, O> {
    out_lats: HashMap<O, u64>,
    inputs: HashMap<I, Port<N, O>>,
}

impl<N, I, O> Default for Node<N, I, O> {
    fn default() -> Self {
        Self {
            out_lats: HashMap::default(),
            inputs: HashMap::default(),
        }
    }
}

impl<N, I, O> PartialEq for Node<N, I, O>
where
    N: Hash + Eq,
    I: Hash + Eq,
    O: Hash + Eq,
{
    fn eq(&self, other: &Self) -> bool {
        self.out_lats == other.out_lats && self.inputs == other.inputs
    }
}

impl<N, I, O> Eq for Node<N, I, O>
where
    N: Hash + Eq,
    I: Hash + Eq,
    O: Hash + Eq,
{
}

impl<N, I: Hash + Eq, O> Node<N, I, O> {
    #[inline]
    pub fn get_port_mut<Q>(&mut self, id: &Q) -> Option<&mut Port<N, O>>
    where
        Q: ?Sized + Hash + Eq,
        I: borrow::Borrow<Q>,
    {
        self.inputs.get_mut(id)
    }

    #[inline]
    pub fn remove_input<Q>(&mut self, id: &Q) -> Option<(I, Port<N, O>)>
    where
        Q: ?Sized + Hash + Eq,
        I: borrow::Borrow<Q>,
    {
        self.inputs.remove_entry(id)
    }

    #[inline]
    pub fn add_input(&mut self, id: I) {
        self.inputs.insert(id, Port::default());
    }
}

impl<N, I, O: Hash + Eq> Node<N, I, O> {
    #[inline]
    pub fn get_latency_mut<Q>(&mut self, id: &Q) -> Option<&mut u64>
    where
        Q: ?Sized + Hash + Eq,
        O: borrow::Borrow<Q>,
    {
        self.out_lats.get_mut(id)
    }

    #[inline]
    pub fn remove_output<Q>(&mut self, id: &Q) -> Option<(O, u64)>
    where
        Q: ?Sized + Hash + Eq,
        O: borrow::Borrow<Q>,
    {
        self.out_lats.remove_entry(id)
    }

    #[inline]
    pub fn add_output(&mut self, id: O) {
        self.add_output_with_latency(id, 0);
    }

    #[inline]
    pub fn add_output_with_latency(&mut self, id: O, latency: u64) {
        self.out_lats.insert(id, latency);
    }
}

impl<N, I, O> Node<N, I, O> {
    #[inline]
    #[must_use]
    pub fn output_latencies(&self) -> &HashMap<O, u64> {
        &self.out_lats
    }

    #[inline]
    #[must_use]
    pub fn input_ports(&self) -> &HashMap<I, Port<N, O>> {
        &self.inputs
    }
}

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
    pub fn can_insert_edge<K, L, Q, R>(&mut self, from: (&K, &L), to: (&Q, &R)) -> bool
    where
        N: borrow::Borrow<K> + borrow::Borrow<Q>,
        I: Hash + Eq + borrow::Borrow<R>,
        O: Hash + Eq + borrow::Borrow<L>,
        Q: ?Sized + Hash + Eq,
        R: ?Sized + Hash + Eq,
        K: ?Sized + Hash + Eq,
        L: ?Sized + Hash + Eq,
    {
        let (source_node, source_output) = from;
        let (dest_node, dest_input) = to;

        self.get_node(dest_node)
            .is_some_and(|n| n.input_ports().contains_key(dest_input))
            && self
                .get_node(source_node)
                .is_some_and(|n| n.output_latencies().contains_key(source_output))
    }

    #[inline]
    pub fn can_insert_edge_acyclic<K, Q, L, R>(
        &mut self,
        from: (&K, &L),
        to: (&Q, &R),
    ) -> Result<(), bool>
    where
        N: borrow::Borrow<Q> + borrow::Borrow<K>,
        I: Hash + Eq + borrow::Borrow<R>,
        O: Hash + Eq + borrow::Borrow<L>,
        Q: ?Sized + Hash + Eq,
        K: ?Sized + Hash + Eq + PartialEq<Q>,
        R: ?Sized + Hash + Eq,
        L: ?Sized + Hash + Eq,
    {
        if self.can_insert_edge(from, to) {
            if self.is_connected::<K, Q>(from.0, to.0) {
                Err(true)
            } else {
                Ok(())
            }
        } else {
            Err(false)
        }
    }

    #[inline]
    pub fn try_insert_edge_acyclic<Q, R>(
        &mut self,
        from: (N, O),
        to: (&Q, &R),
    ) -> Result<bool, bool>
    where
        N: borrow::Borrow<Q>,
        I: Hash + Eq + borrow::Borrow<R>,
        O: Hash + Eq,
        Q: ?Sized + Hash + Eq,
        R: ?Sized + Hash + Eq,
    {
        let (source_node, source_output) = from;
        let (dest_node, dest_input) = to;

        self.can_insert_edge_acyclic(
            (source_node.borrow(), &source_output),
            (dest_node, dest_input),
        )
        .map(|()| {
            self.get_node_mut(dest_node)
                .unwrap()
                .get_port_mut(dest_input)
                .unwrap()
                .insert_connection(source_node, source_output)
        })
    }

    /// # Panics
    ///
    /// If no node with id `from` exists.
    ///
    /// Otherwise, If no node with id `to` exists, this returns false
    fn is_connected<Q, R>(&self, from: &Q, to: &R) -> bool
    where
        N: borrow::Borrow<Q>,
        Q: ?Sized + Hash + Eq + PartialEq<R>,
        R: ?Sized,
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
