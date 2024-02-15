use core::{
    cell::Cell,
    ops::{Deref, DerefMut},
};

use simd_util::enclosing_div;
use voice_manager::{VoiceManager, VoiceUpdate, VoiceUpdateInfo};

use super::*;

pub struct PolyProcessor<T, const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    main_buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
    scratch_buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
    processor: T,
    input_buf_indices: Box<[Option<BufferIndex>]>,
    output_buf_indices: Box<[Option<OutputBufferIndex>]>,
    voice_manager: VoiceManager<N>,
}

impl<const N: usize, T: Processor<N>> PolyProcessor<T, N>
where
    LaneCount<N>: SupportedLaneCount,
{
    pub fn new(processor: T) -> Self {
        let (i, o) = processor.audio_io_layout();

        let main_buffers: Box<_> = iter::repeat_with(|| new_v_float_buffer(0))
            .take(o)
            .collect();

        let scratch_buffers: Box<_> = iter::repeat_with(|| new_v_float_buffer(0))
            .take(o)
            .collect();

        let output_buf_indices = (0..o)
            .map(OutputBufferIndex::Intermediate)
            .map(Some)
            .collect();

        let input_buf_indices = iter::repeat(None).take(i).collect();

        Self {
            main_buffers,
            scratch_buffers,
            processor,
            input_buf_indices,
            output_buf_indices,
            voice_manager: VoiceManager::default(),
        }
    }

    pub fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_polyphony: usize) {
        self.processor
            .initialize(sr, max_buffer_size, enclosing_div(max_polyphony, N / 2));
        [&mut self.main_buffers, &mut self.scratch_buffers]
            .into_iter()
            .for_each(|bufs| {
                bufs.iter_mut()
                    .for_each(|buf| *buf = new_v_float_buffer(max_buffer_size))
            });
    }

    pub fn reset(&mut self) {
        self.processor.reset();
    }

    fn update_voices(&mut self, voice_update: VoiceUpdateInfo) {
        if let Some(update) = voice_update.update {
            match update {
                VoiceUpdate::Add {
                    voice_index: (cluster_idx, voice_idx),
                    midi_note,
                } => {
                    self.processor
                        .activate_voice(cluster_idx, voice_idx, midi_note);
                }
                VoiceUpdate::Remove {
                    voice_index: (cluster_idx, voice_idx),
                } => {
                    self.processor.deactivate_voice(cluster_idx, voice_idx);
                }
            }
        }

        if let Some((from, to)) = voice_update.move_voice {
            self.processor.move_state(from, to);
        }
    }

    pub fn add_voice(&mut self, midi_note: u8) {
        let update = self.voice_manager.add_voice(midi_note);
        self.update_voices(update);
    }

    pub fn remove_voice(&mut self, midi_note: u8) {
        let update = self.voice_manager.remove_voice(midi_note);
        self.update_voices(update);
    }

    pub fn processor(&self) -> &T {
        &self.processor
    }

    pub fn processor_mut(&mut self) -> &mut T {
        &mut self.processor
    }

    pub fn process(&mut self, start: usize, len: NonZeroUsize) {
        let mut active_clusters_idxs = self.voice_manager.active_clusters();

        if let Some(cluster_idx) = active_clusters_idxs.next() {
            self.processor.process(
                Buffers::new(
                    start,
                    len,
                    BufferHandle::toplevel(self.main_buffers.as_ref()),
                    self.input_buf_indices.as_ref(),
                    self.output_buf_indices.as_ref(),
                ),
                cluster_idx,
            );
        } else {
            return;
        }

        for cluster_idx in active_clusters_idxs {
            self.processor.process(
                Buffers::new(
                    start,
                    len,
                    BufferHandle::toplevel(self.scratch_buffers.as_ref()),
                    self.input_buf_indices.as_ref(),
                    self.output_buf_indices.as_ref(),
                ),
                cluster_idx,
            );

            self.main_buffers
                .iter()
                .map(Deref::deref)
                .map(Cell::as_slice_of_cells)
                .zip(
                    self.scratch_buffers
                        .iter()
                        .map(Deref::deref)
                        .map(Cell::as_slice_of_cells),
                )
                .for_each(|(main, scratch)| {
                    for (main_sample, scratch_sample) in main[start..start + len.get()]
                        .iter()
                        .zip(scratch[start..start + len.get()].iter())
                    {
                        main_sample.set(main_sample.get() + scratch_sample.get());
                    }
                });
        }
    }

    pub fn get_buffer_aliased(&self, index: usize) -> Option<&Cell<[Simd<f32, N>]>> {
        self.main_buffers.get(index).map(Deref::deref)
    }

    pub fn get_buffer_exclusive(&mut self, index: usize) -> Option<&mut [Simd<f32, N>]> {
        self.main_buffers
            .get_mut(index)
            .map(DerefMut::deref_mut)
            .map(Cell::get_mut)
    }
}
