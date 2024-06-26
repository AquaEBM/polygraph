extern crate alloc;

use core::{iter, num::NonZeroUsize, ops::AddAssign};

use super::{
    buffer::{
        new_vfloat_buffer, Buffer, BufferHandleLocal, BufferIndex, Buffers, OutputBufferIndex,
    },
    processor::Processor,
    simd_util::{simd::num::SimdFloat, MaskAny, MaskSelect},
    voice::{VoiceEvent, VoiceManager},
};

pub struct StandaloneProcessor<T: Processor, V> {
    output_buf_indices: Box<[Option<OutputBufferIndex>]>,
    max_num_clusters: usize,
    main_bufs: Box<[Buffer<T::Sample>]>,
    scratch_bufs: Box<[Buffer<T::Sample>]>,
    processor: T,
    vm: V,
    events_buffer: Vec<VoiceEvent<T::Sample>>,
}

impl<T: Processor + Default, V: Default> Default for StandaloneProcessor<T, V> {
    fn default() -> Self {
        let processor = T::default();

        let (_, o) = processor.audio_io_layout();

        let empty_buf = || new_vfloat_buffer::<T::Sample>(0);

        let main_bufs = iter::repeat_with(empty_buf).take(o).collect();
        let scratch_bufs = iter::repeat_with(empty_buf).take(o).collect();

        let output_buf_indices = (0..o).map(OutputBufferIndex::Local).map(Some).collect();

        Self {
            output_buf_indices,
            max_num_clusters: 0,
            main_bufs,
            scratch_bufs,
            processor,
            vm: V::default(),
            events_buffer: Vec::with_capacity(2048),
        }
    }
}

impl<T, V> StandaloneProcessor<T, V>
where
    T: Processor,
    V: VoiceManager<T::Sample>,
{
    pub fn note_on(&mut self, note: u8, vel: f32) {
        self.vm.note_on(note, vel)
    }

    pub fn note_off(&mut self, note: u8, vel: f32) {
        self.vm.note_off(note, vel)
    }

    pub fn note_free(&mut self, note: u8) {
        self.vm.note_free(note)
    }

    fn buffer_handle<'a>(
        bufs: &'a mut [Buffer<T::Sample>],
        input_indices: &'a [Option<BufferIndex>],
        output_indices: &'a [Option<OutputBufferIndex>],
        start: usize,
        num_samples: NonZeroUsize,
    ) -> Buffers<'a, T::Sample> {
        BufferHandleLocal::toplevel(bufs)
            .with_indices(input_indices, output_indices)
            .with_buffer_pos(start, num_samples)
    }

    pub fn process(&mut self, current_sample: usize, num_samples: NonZeroUsize)
    where
        <T::Sample as SimdFloat>::Mask: Clone + MaskAny,
        T::Sample: AddAssign + Default + MaskSelect,
    {
        self.vm.flush_events(&mut self.events_buffer);

        for event in self.events_buffer.drain(..) {
            match event {
                VoiceEvent::Activate {
                    note,
                    velocity,
                    cluster_idx,
                    mask,
                } => {
                    self.processor.reset(cluster_idx, mask.clone(), &());
                    self.processor
                        .set_voice_notes(cluster_idx, mask, velocity, note);
                }

                VoiceEvent::Deactivate {
                    velocity,
                    cluster_idx,
                    mask,
                } => {
                    self.processor
                        .deactivate_voices(cluster_idx, mask, velocity);
                }

                VoiceEvent::Move { from, to } => self.processor.move_state(from, to),
            };
        }

        let mut cluster_idxs = (0..self.max_num_clusters).filter_map(|cluster_idx| {
            let mask = self.vm.get_voice_mask(cluster_idx);
            mask.clone().any().then_some((cluster_idx, mask))
        });

        let range = current_sample..current_sample + num_samples.get();
        let zero = T::Sample::default();

        let Some((first_cluster_idx, first_mask)) = cluster_idxs.next() else {
            for buf in self.main_bufs.iter_mut() {
                for sample in &mut buf.get_mut()[range.clone()] {
                    *sample = zero;
                }
            }
            return;
        };

        self.processor.process(
            Self::buffer_handle(
                &mut self.main_bufs,
                &[],
                &self.output_buf_indices,
                current_sample,
                num_samples,
            ),
            first_cluster_idx,
            &(),
        );

        for buf in self.main_bufs.iter_mut() {
            for sample in &mut buf.as_mut().get_mut()[range.clone()] {
                *sample = sample.select_or(first_mask.clone(), zero);
            }
        }

        for (cluster_idx, mask) in cluster_idxs {
            self.processor.process(
                Self::buffer_handle(
                    &mut self.scratch_bufs,
                    &[],
                    &self.output_buf_indices,
                    current_sample,
                    num_samples,
                ),
                cluster_idx,
                &(),
            );

            for (main_buf, scratch_buf) in
                self.main_bufs.iter_mut().zip(self.scratch_bufs.iter_mut())
            {
                for (main_sample, scratch_sample) in main_buf.get_mut()[range.clone()]
                    .iter_mut()
                    .zip(scratch_buf.get_mut()[range.clone()].iter_mut())
                {
                    *main_sample += scratch_sample.select_or(mask.clone(), zero);
                }
            }
        }
    }

    pub fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {
        self.processor
            .initialize(sr, max_buffer_size, max_num_clusters);

        self.vm.set_max_polyphony(max_num_clusters);

        for buf in self
            .main_bufs
            .iter_mut()
            .chain(self.scratch_bufs.iter_mut())
        {
            *buf = new_vfloat_buffer(max_buffer_size);
        }

        self.max_num_clusters = max_num_clusters;
    }

    pub fn get_buffers(&mut self) -> &mut [Buffer<T::Sample>] {
        self.main_bufs.as_mut()
    }

    pub fn processor(&self) -> &T {
        &self.processor
    }

    pub fn processor_mut(&mut self) -> &mut T {
        &mut self.processor
    }
}
