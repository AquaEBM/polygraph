use super::*;

#[derive(Clone)]
pub struct Port<N, O>(HashMap<N, HashSet<O>>);

impl<N: fmt::Debug, O: fmt::Debug> fmt::Debug for Port<N, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Port(")?;
        fmt::Debug::fmt(&self.0, f)?;
        f.write_str(")")
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
