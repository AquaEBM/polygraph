use plugin_util::simd::{LaneCount, Simd, SupportedLaneCount};

use crate::buffer::new_owned_buffer;

use super::{
    audio_graph::{AudioGraph, ProcessTask},
    buffer::{BufferHandle, BufferIndex, Buffers, OutputBufferIndex, OwnedBuffer},
};

use core::{iter, mem, num::NonZeroUsize};

pub mod poly_processor;
mod voice_manager;

#[allow(unused_variables)]
pub trait Processor<const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn audio_io_layout(&self) -> (usize, usize) {
        (0, 0)
    }

    fn process(&mut self, buffers: Buffers<Simd<f32, N>>, cluster_idx: usize) {}

    fn update_param_smoothers(&mut self, num_samples: NonZeroUsize) {}

    fn initialize(&mut self, sr: f32, max_buffer_size: usize) {}

    fn reset(&mut self) {}

    fn set_max_polyphony(&mut self, num_clusters: usize) {}

    fn activate_cluster(&mut self, index: usize) {}

    fn deactivate_cluster(&mut self, index: usize) {}

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {}

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {}

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {}
}

struct Empty;

impl<const N: usize> Processor<N> for Empty where LaneCount<N>: SupportedLaneCount {}

pub(crate) fn new_v_float_buffer<const N: usize>(len: usize) -> OwnedBuffer<Simd<f32, N>>
where
    LaneCount<N>: SupportedLaneCount,
{
    // SAFETY: f32s and thus Simd<f32, N>s are safely zeroable
    unsafe { new_owned_buffer(len) }
}

pub(crate) struct AudioGraphProcessor<const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    processors: Vec<Box<dyn Processor<N>>>,
    schedule: Vec<ProcessTask>,
    buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
    layout: (usize, usize),
}

impl<const N: usize> Default for AudioGraphProcessor<N>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn default() -> Self {
        Self {
            processors: Default::default(),
            schedule: Default::default(),
            buffers: Default::default(),
            layout: Default::default(),
        }
    }
}

impl<const N: usize> AudioGraphProcessor<N>
where
    LaneCount<N>: SupportedLaneCount,
{
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_layout(&mut self, num_inputs: usize, num_outputs: usize) {
        self.layout = (num_inputs, num_outputs);
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

    pub(crate) fn replace_processor(
        &mut self,
        index: usize,
        processor: Box<dyn Processor<N>>,
    ) -> Box<dyn Processor<N>> {
        self.processors
            .get_mut(index)
            .map(|proc_ref| mem::replace(proc_ref, processor))
            .unwrap_or_else(|| Box::new(Empty))
    }

    pub(crate) fn pour_processors_into(
        &mut self,
        mut vec: Vec<Box<dyn Processor<N>>>,
    ) -> Vec<Box<dyn Processor<N>>> {
        debug_assert!(vec.is_empty());
        debug_assert!(vec.capacity() >= self.processors.len());
        for proc in self.processors.drain(..) {
            vec.push(proc);
        }
        mem::replace(&mut self.processors, vec)
    }

    pub(crate) fn remove_processor(&mut self, index: usize) -> Box<dyn Processor<N>> {

        self.replace_processor(index, Box::new(Empty))
    }

    pub(crate) fn schedule_for(&mut self, graph: &AudioGraph, buffer_size: usize) {
        let (schedule, num_buffers) = graph.compile();

        self.replace_schedule(schedule);

        self.replace_buffers(
            iter::repeat_with(|| new_v_float_buffer(buffer_size))
                .take(num_buffers)
                .collect(),
        );
    }
}

impl<const N: usize> Processor<N> for AudioGraphProcessor<N>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn audio_io_layout(&self) -> (usize, usize) {
        self.layout
    }

    fn process(&mut self, buffers: Buffers<Simd<f32, N>>, cluster_idx: usize) {
        let len = buffers.buffer_size().get();
        let start = buffers.start();

        for task in &self.schedule {
            let buffer_handle = BufferHandle::parented(self.buffers.as_mut(), buffers.indices());

            match task {
                ProcessTask::Add {
                    left_input,
                    right_input,
                    output,
                } => {
                    let l = buffer_handle
                        .get_input_buffer(*left_input, start, len)
                        .unwrap();
                    let r = buffer_handle
                        .get_input_buffer(*right_input, start, len)
                        .unwrap();
                    let output = buffer_handle
                        .get_output_buffer(*output, start, len)
                        .unwrap();

                    output.add(l, r);
                }

                ProcessTask::Copy { input, outputs } => {
                    let input = buffer_handle.get_input_buffer(*input, start, len).unwrap();

                    outputs.iter().for_each(|&index| {
                        buffer_handle
                            .get_output_buffer(index, start, len)
                            .unwrap()
                            .copy(input)
                    })
                }

                ProcessTask::Process {
                    index,
                    inputs,
                    outputs,
                } => {
                    let bufs = Buffers::new(
                        buffers.start(),
                        buffers.buffer_size(),
                        buffer_handle,
                        inputs.as_ref(),
                        outputs.as_ref(),
                    );
                    self.processors[*index].process(bufs, cluster_idx);
                }
            }
        }
    }

    fn update_param_smoothers(&mut self, num_samples: NonZeroUsize) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.update_param_smoothers(num_samples));
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize) {
        self.buffers
            .iter_mut()
            .for_each(|buf| *buf = new_v_float_buffer(max_buffer_size));

        self.processors
            .iter_mut()
            .for_each(|proc| proc.initialize(sr, max_buffer_size))
    }

    fn reset(&mut self) {
        self.processors
            .iter_mut()
            .for_each(Processor::reset)
    }

    fn set_max_polyphony(&mut self, num_clusters: usize) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.set_max_polyphony(num_clusters))
    }

    fn activate_cluster(&mut self, index: usize) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.activate_cluster(index))
    }

    fn deactivate_cluster(&mut self, index: usize) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.deactivate_cluster(index))
    }

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.activate_voice(cluster_idx, voice_idx, note))
    }

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.deactivate_voice(cluster_idx, voice_idx))
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.processors
            .iter_mut()
            .for_each(|proc| proc.move_state(from, to))
    }
}

impl<const N: usize, T: ?Sized + Processor<N>> Processor<N> for Box<T>
where
    LaneCount<N>: SupportedLaneCount,
{
    fn audio_io_layout(&self) -> (usize, usize) {
        self.as_ref().audio_io_layout()
    }

    fn process(&mut self, buffers: Buffers<Simd<f32, N>>, cluster_idx: usize) {
        self.as_mut().process(buffers, cluster_idx);
    }

    fn update_param_smoothers(&mut self, num_samples: NonZeroUsize) {
        self.as_mut().update_param_smoothers(num_samples);
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize) {
        self.as_mut().initialize(sr, max_buffer_size);
    }

    fn reset(&mut self) {
        self.as_mut().reset();
    }

    fn set_max_polyphony(&mut self, num_clusters: usize) {
        self.as_mut().set_max_polyphony(num_clusters);
    }

    fn activate_cluster(&mut self, index: usize) {
        self.as_mut().activate_cluster(index);
    }

    fn deactivate_cluster(&mut self, index: usize) {
        self.as_mut().deactivate_cluster(index);
    }

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {
        self.as_mut().activate_voice(cluster_idx, voice_idx, note);
    }

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {
        self.as_mut().deactivate_voice(cluster_idx, voice_idx);
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.as_mut().move_state(from, to);
    }
}
