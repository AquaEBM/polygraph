use simd_util::simd::{LaneCount, Simd, SupportedLaneCount};

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

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {}

    fn set_param_smoothed(&mut self, cluster_idx: usize, param_id: u64, norm_val: Simd<f32, N>) {}

    fn set_param(&mut self, cluster_idx: usize, param_id: u64, norm_val: Simd<f32, N>) {}

    fn reset(&mut self) {}

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

pub struct AudioGraphProcessor<const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
{
    processors: Box<[Option<Box<dyn Processor<N>>>]>,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_layout(&mut self, num_inputs: usize, num_outputs: usize) {
        self.layout = (num_inputs, num_outputs);
    }

    pub fn replace_schedule(&mut self, schedule: Vec<ProcessTask>) -> Vec<ProcessTask> {
        mem::replace(&mut self.schedule, schedule)
    }

    pub fn replace_buffers(
        &mut self,
        buffers: Box<[OwnedBuffer<Simd<f32, N>>]>,
    ) -> Box<[OwnedBuffer<Simd<f32, N>>]> {
        mem::replace(&mut self.buffers, buffers)
    }

    pub fn replace_processor(
        &mut self,
        index: usize,
        processor: Box<dyn Processor<N>>,
    ) -> Option<Box<dyn Processor<N>>> {
        self.processors
            .get_mut(index)
            .and_then(Option::as_mut)
            .map(|proc| mem::replace(proc, processor))
    }

    pub fn pour_processors_into(
        &mut self,
        mut list: Box<[Option<Box<dyn Processor<N>>>]>,
    ) -> Box<[Option<Box<dyn Processor<N>>>]> {
        debug_assert!(list.len() >= self.processors.len());
        for (input, output) in self.processors.iter_mut().zip(list.iter_mut()) {
            mem::swap(input, output);
        }
        mem::replace(&mut self.processors, list)
    }

    pub fn remove_processor(&mut self, index: usize) -> Option<Box<dyn Processor<N>>> {
        self.processors.get_mut(index).and_then(Option::take)
    }

    pub fn schedule_for(&mut self, graph: &AudioGraph, buffer_size: usize) {
        let (schedule, num_buffers) = graph.compile();

        self.replace_schedule(schedule);

        self.replace_buffers(
            iter::repeat_with(|| new_v_float_buffer(buffer_size))
                .take(num_buffers)
                .collect(),
        );
    }

    pub fn processors(&mut self) -> impl Iterator<Item = &mut (dyn Processor<N> + 'static)> {
        self.processors.iter_mut().filter_map(Option::as_deref_mut)
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
                    self.processors[*index]
                        .as_deref_mut()
                        .unwrap()
                        .process(bufs, cluster_idx);
                }
            }
        }
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {
        self.buffers
            .iter_mut()
            .for_each(|buf| *buf = new_v_float_buffer(max_buffer_size));

        self.processors()
            .for_each(|proc| proc.initialize(sr, max_buffer_size, max_num_clusters))
    }

    fn reset(&mut self) {
        self.processors().for_each(Processor::reset)
    }

    fn activate_voice(&mut self, cluster_idx: usize, voice_idx: usize, note: u8) {
        self.processors()
            .for_each(|proc| proc.activate_voice(cluster_idx, voice_idx, note))
    }

    fn deactivate_voice(&mut self, cluster_idx: usize, voice_idx: usize) {
        self.processors()
            .for_each(|proc| proc.deactivate_voice(cluster_idx, voice_idx))
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.processors().for_each(|proc| proc.move_state(from, to))
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

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {
        self.as_mut()
            .initialize(sr, max_buffer_size, max_num_clusters);
    }

    fn set_param(&mut self, cluster_idx: usize, param_id: u64, norm_val: Simd<f32, N>) {
        self.as_mut().set_param(cluster_idx, param_id, norm_val);
    }

    fn set_param_smoothed(&mut self, cluster_idx: usize, param_id: u64, norm_val: Simd<f32, N>) {
        self.as_mut()
            .set_param_smoothed(cluster_idx, param_id, norm_val);
    }

    fn reset(&mut self) {
        self.as_mut().reset();
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
