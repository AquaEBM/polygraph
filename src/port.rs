use super::*;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct InputID(NonZeroU32);

impl Debug for InputID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.0)
    }
}

impl InputID {
    #[inline]
    pub(crate) fn new_key(map: &HashMap<Self, impl Sized>) -> Self {
        let mut id = Self(NonZeroU32::MIN);

        while map.contains_key(&id) {
            id.0 = id.0.checked_add(1).expect("Index overflow");
        }

        id
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct OutputID(NonZeroU32);

impl Debug for OutputID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.0)
    }
}

impl OutputID {
    #[inline]
    pub(crate) fn new_key(map: &HashMap<Self, impl Sized>) -> Self {
        let mut id = Self(NonZeroU32::MIN);

        while map.contains_key(&id) {
            id.0 = id.0.checked_add(1).expect("Index overflow");
        }

        id
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct NodeID(NonZeroU32);

impl Debug for NodeID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // to override pretty-print
        write!(f, "{:?}", &self.0)
    }
}

impl NodeID {
    #[inline]
    pub(crate) fn new_key(map: &HashMap<Self, impl Sized>) -> Self {
        let mut id = Self(NonZeroU32::MIN);

        while map.contains_key(&id) {
            id.0 = id.0.checked_add(1).expect("Index overflow");
        }

        id
    }
}

#[derive(Clone)]
pub struct Port<P = OutputID>(HashMap<NodeID, HashSet<P>>);

impl<P: Debug> Debug for Port<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.0)
    }
}

impl<P: Hash + Eq> PartialEq for Port<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P> Default for Port<P> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

impl<P: Hash + Eq> Eq for Port<P> {}

impl<P> Port<P> {
    #[inline]
    #[must_use]
    pub fn connections(&self) -> &HashMap<NodeID, HashSet<P>> {
        &self.0
    }
}

impl<P> Port<P> {
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
    pub fn iter_connections(&self) -> impl Iterator<Item = (&NodeID, &P)> {
        self.0
            .iter()
            .flat_map(|(node_id, ports)| ports.iter().map(move |port| (node_id, port)))
    }
}

impl<P: Hash + Eq> Port<P> {
    #[inline]
    pub(crate) fn insert_connection(&mut self, node_index: NodeID, port_index: P) -> bool {
        self.0.entry(node_index).or_default().insert(port_index)
    }

    #[inline]
    pub fn remove_port(&mut self, node_index: &NodeID, port_index: &P) -> bool {
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
