use simd_util::{simd::{f32x2, num::SimdFloat}, splat_stereo, Float};

use super::{
    audio_graph::{AudioGraph, ProcessTask},
    buffer::{BufferHandle, Buffers, OwnedBuffer, new_owned_buffer, BufferIndices},
};

use core::{any::Any, iter, mem, ops::Add};

pub struct Params(pub Box<[Float]>);

impl Params {
    pub fn get_param(&self, param_id: u64) -> Option<Float> {
        self.0.get(param_id as usize).copied()
    }
}

#[allow(unused_variables)]
pub trait Processor {
    type Sample: SimdFloat;

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
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        param_id: u64,
        norm_val: Self::Sample,
    ) {
    }

    fn set_all_params(
        &mut self,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        params: &Params,
    ) {
    }

    fn set_voice_note(
        &mut self,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        note: Self::Sample,
    ) {
    }

    fn custom_event(&mut self, event: &mut dyn Any) {}

    fn reset(&mut self, cluster_idx: usize, voice_mask: &<Self::Sample as SimdFloat>::Mask) {}

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {}
}

pub fn new_vfloat_buffer<T: SimdFloat>(len: usize) -> OwnedBuffer<T> {
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
            iter::repeat_with(|| new_vfloat_buffer(buffer_size))
                .take(num_buffers)
                .collect(),
        );
    }

    pub fn processors(&mut self) -> impl Iterator<Item = &mut T> {
        self.processors.iter_mut().filter_map(Option::as_mut)
    }
}

impl<T: Processor> Processor for AudioGraphProcessor<T>
where
    T::Sample: Add<Output = T::Sample>,
{
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
            let handle = BufferHandle::parented(self.buffers.as_mut(), buffers.indices());

            match task {
                ProcessTask::Add {
                    left_input,
                    right_input,
                    output,
                } => {
                    let l = handle.get_input_buffer(*left_input, start, len).unwrap();
                    let r = handle.get_input_buffer(*right_input, start, len).unwrap();
                    let output = handle.get_output_buffer(*output, start, len).unwrap();

                    for ((l, r), output) in l.iter().zip(r).zip(output) {
                        output.set(l.get() + r.get())
                    }
                }

                ProcessTask::Copy { input, outputs } => {
                    let input = handle.get_input_buffer(*input, start, len).unwrap();

                    outputs.iter().for_each(|&index| {
                        for (i, o) in input
                            .iter()
                            .zip(handle.get_output_buffer(index, start, len).unwrap())
                        {
                            o.set(i.get())
                        }
                    })
                }

                ProcessTask::Process {
                    index,
                    inputs,
                    outputs,
                } => {
                    let indices = BufferIndices::new(handle, inputs, outputs);

                    let bufs = Buffers::new(
                        buffers.start(),
                        buffers.buffer_size(),
                        indices,
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
            .for_each(|buf| *buf = new_vfloat_buffer(max_buffer_size));

        self.processors()
            .for_each(|proc| proc.initialize(sr, max_buffer_size, max_num_clusters))
    }

    fn reset(&mut self, cluster_idx: usize, voice_mask: &<Self::Sample as SimdFloat>::Mask) {
        self.processors()
            .for_each(|proc| proc.reset(cluster_idx, voice_mask))
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.processors().for_each(|proc| proc.move_state(from, to))
    }

    fn set_voice_note(
        &mut self,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        note: Self::Sample,
    ) {
        self.processors()
            .for_each(|proc| proc.set_voice_note(cluster_idx, voice_mask, note))
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
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        param_id: u64,
        norm_val: Self::Sample,
    ) {
        self.as_mut()
            .set_param(cluster_idx, voice_mask, param_id, norm_val);
    }

    fn set_all_params(
        &mut self,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        params: &Params,
    ) {
        self.as_mut()
            .set_all_params(cluster_idx, voice_mask, params);
    }

    fn set_voice_note(
        &mut self,
        cluster_idx: usize,
        voice_mask: &<Self::Sample as SimdFloat>::Mask,
        note: Self::Sample,
    ) {
        self.as_mut().set_voice_note(cluster_idx, voice_mask, note);
    }
}
