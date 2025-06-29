use super::*;

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
