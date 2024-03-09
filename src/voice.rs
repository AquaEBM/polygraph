use simd_util::simd::{num::SimdFloat, Simd, SimdElement, LaneCount, SupportedLaneCount};

#[derive(Clone, Copy)]
pub enum VoiceEvent<S: SimdFloat> {
    Activate {
        note: S::Bits,
        velocity: S,
        cluster_idx: usize,
        mask: S::Mask,
    },

    Deactivate {
        velocity: S,
        cluster_idx: usize,
        mask: S::Mask,
    },

    Free {
        cluster_idx: usize,
        mask: S::Mask,
    },

    Move {
        from: (usize, usize),
        to: (usize, usize),
    },
}

pub trait VoiceManager<S: SimdFloat> {
    fn note_on(&mut self, note: u8, vel: f32);
    fn note_off(&mut self, note: u8, vel: f32);
    fn note_free(&mut self, note: u8);
    fn flush_events(&mut self, events: &mut Vec<VoiceEvent<S>>);
    fn set_max_polyphony(&mut self, max_num_clusters: usize);
}

pub struct StackVoiceManager {
    voices: Vec<u8>,
    add_pending: Vec<u8>,
    free_pending: Vec<u8>,
    deactivate_pending: Vec<u8>,
}