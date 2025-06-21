use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    latencies: HashMap<OutputID, u64>,
    ports: HashMap<InputID, Port>,
}

impl Index<&InputID> for Node {
    type Output = Port;

    fn index(&self, id: &InputID) -> &Self::Output {
        self.input_ports()
            .get(id)
            .expect("No input port found for this ID")
    }
}

impl IndexMut<&InputID> for Node {
    fn index_mut(&mut self, id: &InputID) -> &mut Self::Output {
        self.get_port_mut(id)
            .expect("No input port found for this ID")
    }
}

impl Index<&OutputID> for Node {
    type Output = u64;

    fn index(&self, id: &OutputID) -> &Self::Output {
        self.output_latencies()
            .get(id)
            .expect("No output port found for this ID")
    }
}

impl IndexMut<&OutputID> for Node {
    fn index_mut(&mut self, id: &OutputID) -> &mut Self::Output {
        self.get_latency_mut(id)
            .expect("No input port found for this ID")
    }
}

impl Node {
    #[inline]
    #[must_use]
    pub fn input_ports(&self) -> &HashMap<InputID, Port> {
        &self.ports
    }

    #[inline]
    pub fn get_port_mut(&mut self, id: &InputID) -> Option<&mut Port> {
        self.ports.get_mut(id)
    }

    #[inline]
    pub fn remove_input(&mut self, id: &InputID) -> Option<Port> {
        self.ports.remove(id)
    }

    #[inline]
    pub fn add_input(&mut self) -> InputID {
        let k = InputID::new_key(self.input_ports());
        // SAFETY: !self.ports.contains_key(&k)
        self.ports.insert(k, Port::default());
        k
    }

    #[inline]
    #[must_use]
    pub fn output_latencies(&self) -> &HashMap<OutputID, u64> {
        &self.latencies
    }

    #[inline]
    pub fn get_latency_mut(&mut self, id: &OutputID) -> Option<&mut u64> {
        self.latencies.get_mut(id)
    }

    #[inline]
    pub fn remove_output(&mut self, id: &OutputID) -> Option<u64> {
        self.latencies.remove(id)
    }

    #[inline]
    pub fn add_output(&mut self, latency: u64) -> OutputID {
        let k = OutputID::new_key(self.output_latencies());
        // SAFETY: !self.latencies.contains_key(&k)
        self.latencies.insert(k, latency);
        k
    }
}
