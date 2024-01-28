#[derive(Default)]
pub struct VoiceManager<const MAX_VECTOR_WIDTH: usize> {
    notes:  Vec<u8>,
    cap: usize,
}

impl<const V: usize> VoiceManager<V> {

    fn index_to_pos(i: usize) -> (usize, usize) {
        (i / (V / 2), i % (V / 2))
    }

    pub fn add_voice(&mut self, n: u8) -> Option<(usize, usize)> {
        let i = self.notes.len();
        (i < self.cap).then(|| {
            self.notes.push(n);
            Self::index_to_pos(i)
        })
    }

    pub fn remove_voice(&mut self, n: u8) -> Option<(usize, usize)> {
        self.notes.iter().position(|i| i == &n).map(Self::index_to_pos)
    }

    pub fn active_clusters(&self) -> impl Iterator<Item = usize> {
        0..self.notes.len()
    }
}
