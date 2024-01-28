use plugin_util::simd::{Simd, LaneCount, SupportedLaneCount};
use core::mem::transmute;


use super::{
    buffer::{Buffers, OutputBuffer, OwnedBuffer, BufferHandle},
    audio_graph::{ProcessTask, AudioGraph},
};

use core::{num::NonZeroUsize, mem, iter};

mod voice_manager;
mod poly_processor;

#[allow(unused_variables)]
pub trait Processor<const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn audio_io_layout(&self) -> (usize, usize);

    fn process<'a>(
        &mut self,
        buffers: Buffers<Simd<f32, N>>,
        cluster_idx: usize,
    );

    fn update_param_smoothers(&mut self, num_samples: NonZeroUsize) {}

    fn initialize(&mut self, sr: f32, max_buffer_size: usize) {}

    fn reset(&mut self) {}

    fn set_max_polyphony(&mut self, num_cluster: usize) {}

    fn activate_cluster(&mut self, index: usize) {}

    fn deactivate_cluster(&mut self, index: usize) {}

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {}

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {}
}

pub(crate) struct AudioGraphProcessor<const N: usize, const I: usize, const O: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    processors: Vec<Option<Box<dyn Processor<N>>>>,
    schedule: Vec<ProcessTask>,
    buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
}

impl<const N: usize, const I: usize, const O: usize> Default for AudioGraphProcessor<N, I, O>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn default() -> Self {
        Self {
            processors: Default::default(),
            schedule: Default::default(),
            buffers: Default::default(),
        }
    }
}

impl<const N: usize, const I: usize, const O: usize> AudioGraphProcessor<N, I, O>
where
    LaneCount<N>: SupportedLaneCount,
{
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn replace_schedule(&mut self, schedule: Vec<ProcessTask>) -> Vec<ProcessTask> {
        mem::replace(&mut self.schedule, schedule)
    }

    pub(crate) fn replace_buffers(
        &mut self,
        buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
    ) -> Box<[OwnedBuffer<Simd<f32, N>>]> {
        mem::replace(&mut self.buffers, buffers)
    }

    pub(crate) fn replace_processor(&mut self, index: usize, processor: Box<dyn Processor<N>>) -> Option<Box<dyn Processor<N>>> {
        self.processors.get_mut(index).and_then(|maybe_proc| maybe_proc.replace(processor))
    }

    pub(crate) fn insert_processor(&mut self, processor: Box<dyn Processor<N>>) -> usize {
        let proc = Some(processor);

        for (i, slot) in self.processors.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = proc;
                return i;
            }
        }

        let len = self.processors.len();
        self.processors.push(proc);
        len
    }

    pub(crate) fn remove_processor(&mut self, index: usize) -> Option<Box<dyn Processor<N>>> {
        self.processors.get_mut(index).and_then(Option::take)
    }

    pub(crate) fn schedule_for(&mut self, graph: &AudioGraph, buffer_size: usize) {
        let (schedule, num_buffers) = graph.compile();

        self.schedule = schedule;

        self.buffers = iter::repeat_with(||
            // SAFETY: 0 (0.0) is a valid float value, and Cell<T> has the same layout as T
            unsafe { transmute(Box::<[Simd<f32, N>]>::new_zeroed_slice(buffer_size).assume_init()) },
        )
        .take(num_buffers)
        .collect()
    }
}

impl<const N: usize, const I: usize, const O: usize> Processor<N> for AudioGraphProcessor<N, I, O>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn audio_io_layout(&self) -> (usize, usize) {
        (I, O)
    }

    fn process<'a>(
        &mut self,
        buffers: Buffers<Simd<f32, N>>,
        cluster_idx: usize,
    ) {
        let len = buffers.buffer_size().get();

        for task in &self.schedule {

            let buffer_handle = BufferHandle::parented(self.buffers.as_mut(), buffers.indices());

            match task {
                ProcessTask::Add {
                    left_input,
                    right_input,
                    output,
                } => {
                    let l = buffer_handle.get_input_buffer(*left_input, len).unwrap();
                    let r = buffer_handle.get_input_buffer(*right_input, len).unwrap();
                    let output = buffer_handle.get_output_buffer(*output, len).unwrap();

                    output.add(l, r);
                },

                ProcessTask::Copy { input, outputs } => {
                    let input = buffer_handle.get_input_buffer(*input, len).unwrap();

                    outputs.iter().for_each(|&index| buffer_handle
                        .get_output_buffer(index, len).unwrap().copy(input)
                    )
                }

                ProcessTask::Process {
                    index,
                    inputs,
                    outputs,
                } => {
                    let bufs = Buffers::new(
                        buffers.buffer_size(),
                        buffer_handle,
                        inputs.as_ref(),
                        outputs.as_ref(),
                    );
                    self.processors[*index].as_mut().unwrap().process(
                        bufs,
                        cluster_idx,
                    );
                }
            }
        }
    }

    fn update_param_smoothers(&mut self, num_samples: NonZeroUsize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.update_param_smoothers(num_samples));
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.initialize(sr, max_buffer_size))
    }

    fn reset(&mut self) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(Processor::reset)
    }

    fn set_max_polyphony(&mut self, num_clusters: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.set_max_polyphony(num_clusters))
    }

    fn activate_cluster(&mut self, index: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.activate_cluster(index))
    }

    fn deactivate_cluster(&mut self, index: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.deactivate_cluster(index))
    }

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.activate_voice(cluster_idx, voice_idx, note))
    }

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.deactivate_voice(cluster_idx, voice_idx))
    }
}