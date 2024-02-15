#[derive(Default)]
pub(crate) struct VoiceManager<const MAX_VECTOR_WIDTH: usize> {
    notes: Vec<u8>,
    cap: usize,
}

pub enum VoiceUpdate {
    Add {
        midi_note: u8,
        voice_index: (usize, usize),
    },
    Remove {
        voice_index: (usize, usize),
    },
}

pub(crate) struct VoiceUpdateInfo {
    pub update: Option<VoiceUpdate>,
    pub move_voice: Option<((usize, usize), (usize, usize))>,
}

impl<const V: usize> VoiceManager<V> {
    const V: usize = V / 2;

    fn index_to_pos(i: usize) -> (usize, usize) {
        (i / Self::V, i % Self::V)
    }

    pub fn num_active_clusters(&self) -> usize {
        self.num_active_voices() / Self::V
    }

    pub fn num_active_voices(&self) -> usize {
        self.notes.len()
    }

    pub fn add_voice(&mut self, midi_note: u8) -> VoiceUpdateInfo {
        let len = self.num_active_voices();
        VoiceUpdateInfo {
            update: (len < self.cap).then(|| {
                self.notes.push(midi_note);
                VoiceUpdate::Add {
                    midi_note,
                    voice_index: Self::index_to_pos(len),
                }
            }),
            move_voice: None,
        }
    }

    pub fn remove_voice(&mut self, midi_note: u8) -> VoiceUpdateInfo {
        let (update, move_voice) = self
            .notes
            .iter()
            .position(|id| id == &midi_note)
            .map(|index| {
                self.notes.swap_remove(index);

                let voice_index = Self::index_to_pos(index);

                (
                    VoiceUpdate::Remove { voice_index },
                    (Self::index_to_pos(self.num_active_voices()), voice_index),
                )
            })
            .unzip();

        VoiceUpdateInfo { update, move_voice }
    }

    pub fn active_clusters(&self) -> impl Iterator<Item = usize> {
        0..self.notes.len()
    }

    fn set_capacity_voices(&mut self, num_voices: usize) {
        self.notes = Vec::with_capacity(num_voices);
        self.cap = num_voices;
    }

    pub fn set_capacity_clusters(&mut self, num_clusters: usize) {
        self.set_capacity_voices(num_clusters * Self::V);
    }
}
