use simd_util::simd::num::SimdFloat;

use crate::buffer::new_owned_buffer;

use super::{
    audio_graph::{AudioGraph, ProcessTask},
    buffer::{BufferHandle, Buffers, OwnedBuffer},
};

use core::{any::Any, iter, mem, ops::Add};

#[allow(unused_variables)]
pub trait Processor {
    type Sample: SimdFloat + Add<Output = Self::Sample>;

    fn audio_io_layout(&self) -> (usize, usize) {
        (0, 0)
    }

    fn process(
        &mut self,
        buffers: Buffers<Self::Sample>,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
    ) {
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {}

    fn set_param(
        &mut self,
        cluster_idx: usize,
        param_id: u64,
        norm_val: Self::Sample,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        smoothed: bool,
    ) {
    }

    fn custom_event(&mut self, event: &mut dyn Any) {}

    fn reset(&mut self, cluster_idx: usize, voice_mask: &<Self::Sample as SimdFloat>::Mask) {}

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {}
}

pub(crate) fn new_v_float_buffer<T: SimdFloat>(len: usize) -> OwnedBuffer<T> {
    // SAFETY: f32s and thus Simd<f32, N>s are safely zeroable
    unsafe { new_owned_buffer(len) }
}

pub struct AudioGraphProcessor<T: Processor> {
    processors: Box<[Option<T>]>,
    schedule: Vec<ProcessTask>,
    buffers: Box<[OwnedBuffer<T::Sample>]>,
    layout: (usize, usize),
}

impl<T: Processor> Default for AudioGraphProcessor<T> {
    fn default() -> Self {
        Self {
            processors: Default::default(),
            schedule: Default::default(),
            buffers: Default::default(),
            layout: Default::default(),
        }
    }
}

impl<T: Processor> AudioGraphProcessor<T> {
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
        buffers: Box<[OwnedBuffer<T::Sample>]>,
    ) -> Box<[OwnedBuffer<T::Sample>]> {
        mem::replace(&mut self.buffers, buffers)
    }

    pub fn replace_processor(&mut self, index: usize, processor: T) -> Option<T> {
        self.processors
            .get_mut(index)
            .and_then(Option::as_mut)
            .map(|proc| mem::replace(proc, processor))
    }

    pub fn pour_processors_into(&mut self, mut list: Box<[Option<T>]>) -> Box<[Option<T>]> {
        debug_assert!(list.len() >= self.processors.len());
        for (input, output) in self.processors.iter_mut().zip(list.iter_mut()) {
            mem::swap(input, output);
        }
        mem::replace(&mut self.processors, list)
    }

    pub fn remove_processor(&mut self, index: usize) -> Option<T> {
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

    pub fn processors(&mut self) -> impl Iterator<Item = &mut T> {
        self.processors.iter_mut().filter_map(Option::as_mut)
    }
}

impl<T: Processor> Processor for AudioGraphProcessor<T> {
    type Sample = T::Sample;

    fn audio_io_layout(&self) -> (usize, usize) {
        self.layout
    }

    fn process(
        &mut self,
        buffers: Buffers<Self::Sample>,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
    ) {
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
                    self.processors[*index].as_mut().unwrap().process(
                        bufs,
                        cluster_idx,
                        voice_mask,
                    );
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

    fn reset(&mut self, cluster_idx: usize, voice_mask: &<Self::Sample as SimdFloat>::Mask) {
        self.processors()
            .for_each(|proc| proc.reset(cluster_idx, voice_mask));
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.processors().for_each(|proc| proc.move_state(from, to))
    }
}

impl<T: ?Sized + Processor> Processor for Box<T> {
    type Sample = T::Sample;

    fn audio_io_layout(&self) -> (usize, usize) {
        self.as_ref().audio_io_layout()
    }

    fn process(
        &mut self,
        buffers: Buffers<Self::Sample>,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
    ) {
        self.as_mut().process(buffers, cluster_idx, voice_mask);
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) {
        self.as_mut()
            .initialize(sr, max_buffer_size, max_num_clusters);
    }

    fn custom_event(&mut self, event: &mut dyn Any) {
        self.as_mut().custom_event(event);
    }

    fn reset(&mut self, cluster_idx: usize, voice_mask: &<Self::Sample as SimdFloat>::Mask) {
        self.as_mut().reset(cluster_idx, voice_mask);
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.as_mut().move_state(from, to);
    }

    fn set_param(
        &mut self,
        cluster_idx: usize,
        param_id: u64,
        norm_val: Self::Sample,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        smoothed: bool,
    ) {
        self.as_mut()
            .set_param(cluster_idx, param_id, norm_val, voice_mask, smoothed);
    }
}
