use core::mem;

use simd_util::{
    simd::{num::SimdFloat, LaneCount, SupportedLaneCount},
    Float, TMask, UInt,
};

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

pub struct StackVoiceManager<const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    voices: Vec<u8>,
    mask_cache: Vec<TMask<N>>,
    vel_cache: Vec<Float<N>>,
    note_cache: Vec<UInt<N>>,
    add_pending: Vec<(u8, f32)>,
    deactivate_pending: Vec<(u8, f32)>,
    free_pending: Vec<u8>,
}

fn push_within_capacity_stable<T>(vec: &mut Vec<T>, val: T) -> bool {
    let can_push = vec.len() < vec.capacity();
    if can_push {
        vec.push(val)
    }
    can_push
}

impl<const N: usize> VoiceManager<Float<N>> for StackVoiceManager<N>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn note_on(&mut self, note: u8, vel: f32) {
        push_within_capacity_stable(&mut self.add_pending, (note, vel));
    }

    fn note_off(&mut self, note: u8, vel: f32) {
        push_within_capacity_stable(&mut self.deactivate_pending, (note, vel));
    }

    fn note_free(&mut self, note: u8) {
        push_within_capacity_stable(&mut self.free_pending, note);
    }

    fn flush_events(&mut self, events: &mut Vec<VoiceEvent<Float<N>>>) {
        // handle voices scheduled to be deactivated first
        self.deactivate_pending
            .drain(..)
            .filter_map(|(note, vel)| {
                self.voices
                    .iter()
                    .position(|&note_id| note_id == note)
                    .map(|pos| (pos, vel))
            })
            .for_each(|(i, vel)| {
                let v = N / 2;
                let (i, j) = (i / v, i % v);
                let j1 = 2 * j;
                let j2 = j1 + 1;

                let mask = &mut self.mask_cache[i];
                mask.set(j1, true);
                mask.set(j2, true);

                let vels = &mut self.vel_cache[i];
                vels[j1] = vel;
                vels[j2] = vel;
            });

        events.extend(
            self.mask_cache
                .iter_mut()
                .zip(self.vel_cache.iter_mut())
                .enumerate()
                .filter(|(_, (mask, _))| mask.any())
                .map(|(i, (mask, vels))| VoiceEvent::Deactivate {
                    velocity: mem::replace(vels, Float::splat(0.0)),
                    cluster_idx: i,
                    mask: mem::replace(mask, TMask::splat(false)),
                }),
        );

        // then those scheduled to be completely freed
        for note in self.free_pending.drain(..) {
            if let Some(i) = self.voices.iter().position(|&note_id| note_id == note) {
                self.voices[i] = 128;

                while self.voices.last().filter(|&&i| i > 127).is_some() {
                    self.voices.pop();
                }

                let v = N / 2;
                let (i, j) = (i / v, i % v);

                let j1 = 2 * j;
                let j2 = j1 + 1;

                let mask = &mut self.mask_cache[i];
                mask.set(j1, true);
                mask.set(j2, true);
            }
        }

        events.extend(
            self.mask_cache
                .iter_mut()
                .enumerate()
                .filter(|(_, mask)| mask.any())
                .map(|(i, mask)| VoiceEvent::Free {
                    cluster_idx: i,
                    mask: mem::replace(mask, TMask::splat(false)),
                }),
        );

        // fill the gaps with voices scheduled to be activated
        for (note, vel) in self.add_pending.drain(..) {
            if let Some(i) = self
                .voices
                .iter()
                .position(|&note_id| note_id > 127)
                .or_else(|| {
                    let len = self.voices.len();
                    push_within_capacity_stable(&mut self.voices, 128).then_some(len)
                })
            {
                self.voices[i] = note;

                let v = N / 2;
                let (i, j) = (i / v, i % v);
                let j1 = 2 * j;
                let j2 = j1 + 1;

                let mask = &mut self.mask_cache[i];
                mask.set(j1, true);
                mask.set(j2, true);

                let vels = &mut self.vel_cache[i];
                vels[j1] = vel;
                vels[j2] = vel;

                let notes = &mut self.note_cache[i];
                notes[j1] = note.into();
                notes[j2] = note.into();
            }
        }

        events.extend(
            self.note_cache
                .iter_mut()
                .zip(self.vel_cache.iter_mut())
                .zip(self.mask_cache.iter_mut())
                .enumerate()
                .filter(|(_, (_, mask))| mask.any())
                .map(|(i, ((note, vel), mask))| VoiceEvent::Activate {
                    note: mem::replace(note, UInt::splat(0)),
                    velocity: mem::replace(vel, Float::splat(0.0)),
                    cluster_idx: i,
                    mask: mem::replace(mask, TMask::splat(false)),
                }),
        );

        // consolidate voice allocation by moving last voices into the remaining gaps
        let mut i = 0;
        while i < self.voices.len() {
            if self.voices[i] > 127 {
                let len = self.voices.len() - 1;
                self.voices.swap(len, i);
                while self.voices.last().filter(|&&i| i > 127).is_some() {
                    self.voices.pop();
                }

                let v = N / 2;

                events.push(VoiceEvent::Move {
                    from: (len / v, len % v),
                    to: (i / v, i % v),
                });
            }
            i += 1;
        }
    }

    fn set_max_polyphony(&mut self, max_num_clusters: usize) {
        let stereo_voices_per_vector = N / 2;
        let total_num_voices = max_num_clusters * stereo_voices_per_vector;
        self.voices = Vec::with_capacity(total_num_voices);
        self.deactivate_pending = Vec::with_capacity(total_num_voices);
        self.free_pending = Vec::with_capacity(total_num_voices);
        self.add_pending = Vec::with_capacity(total_num_voices);
        self.mask_cache = vec![TMask::splat(false); max_num_clusters];
        self.note_cache = vec![UInt::splat(128); max_num_clusters];
        self.vel_cache = vec![Float::splat(0.0); max_num_clusters];
    }
}
