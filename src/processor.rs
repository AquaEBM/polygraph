use simd_util::{simd::num::SimdFloat, MaskSplat};

use super::{
    audio_graph::{AudioGraph, ProcessTask},
    buffer::{new_vfloat_buffer, Buffer, Buffers, OutputBufferIndex},
};

use alloc::sync::Arc;
use core::{
    cell::Cell,
    iter, mem,
    ops::{Add, BitAndAssign},
};
use std::io::{Read, Write};

pub struct ParameterMut<'a, T: SimdFloat> {
    value: &'a mut T,
    mod_state: &'a mut T::Mask,
    changed: &'a mut bool,
}

impl<'a, T: SimdFloat> ParameterMut<'a, T> {
    pub fn new(value: &'a mut T, mod_state: &'a mut T::Mask, changed: &'a mut bool) -> Self {
        Self {
            value,
            mod_state,
            changed,
        }
    }

    pub fn set_value(&mut self, val: T) {
        *self.value = val;
        *self.changed = true;
    }

    pub fn changed(&self) -> bool {
        *self.changed
    }

    pub fn mod_state(&self) -> &T::Mask {
        self.mod_state
    }

    pub fn set_mod_state(&mut self, mask: T::Mask) {
        *self.mod_state = mask;
    }
}

pub struct Parameter<'a, T: SimdFloat> {
    value: &'a T,
    mod_state: &'a T::Mask,
    changed: &'a Cell<bool>,
}

impl<'a, T: SimdFloat> Parameter<'a, T> {
    pub fn new(value: &'a T, mod_state: &'a T::Mask, changed: &'a Cell<bool>) -> Self {
        Self {
            value,
            mod_state,
            changed,
        }
    }

    pub fn value(&self) -> &'a T {
        self.value
    }

    pub fn changed(&self) -> bool {
        self.changed.get()
    }

    pub fn value_if_changed(&self) -> Option<&'a T> {
        self.changed.take().then_some(self.value)
    }

    pub fn mod_state(&self) -> &'a T::Mask {
        self.mod_state
    }
}

pub trait Parameters<T: SimdFloat> {
    fn get(&self, id: u64) -> Option<Parameter<T>>;
    fn get_mut(&mut self, id: u64) -> Option<ParameterMut<T>>;
}

impl<T: SimdFloat> Parameters<T> for () {
    fn get(&self, _id: u64) -> Option<Parameter<T>> {
        None
    }
    fn get_mut(&mut self, _id: u64) -> Option<ParameterMut<T>> {
        None
    }
}

pub trait PersistentState {
    fn ser(&self, writer: &mut dyn Write);
    fn de(&self, reader: &mut dyn Read);
}

impl PersistentState for () {
    fn ser(&self, _writer: &mut dyn Write) {}
    fn de(&self, _reader: &mut dyn Read) {}
}

#[allow(unused_variables)]
pub trait Processor {
    type Sample: SimdFloat;

    fn audio_io_layout(&self) -> (usize, usize) {
        (0, 0)
    }

    fn persistent_state_handle(&self) -> Arc<dyn PersistentState> {
        Arc::new(())
    }

    fn process(
        &mut self,
        buffers: Buffers<Self::Sample>,
        cluster_idx: usize,
        params: &dyn Parameters<Self::Sample>,
    ) -> <Self::Sample as SimdFloat>::Mask;

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) -> usize {
        0
    }

    fn set_voice_notes(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
        note: <Self::Sample as SimdFloat>::Bits,
    ) {
    }

    fn deactivate_voices(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
    ) {
    }

    fn reset(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        params: &dyn Parameters<Self::Sample>,
    ) {
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {}
}

pub struct AudioGraphProcessor<T: Processor> {
    processors: Box<[Option<T>]>,
    schedule: Vec<ProcessTask>,
    buffers: Box<[Buffer<T::Sample>]>,
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
        buffers: Box<[Buffer<T::Sample>]>,
    ) -> Box<[Buffer<T::Sample>]> {
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

impl<T> Processor for AudioGraphProcessor<T>
where
    T: Processor,
    T::Sample: Add<Output = T::Sample>,
    <T::Sample as SimdFloat>::Mask: Clone + BitAndAssign + MaskSplat,
    <T::Sample as SimdFloat>::Bits: Clone,
{
    type Sample = T::Sample;

    fn audio_io_layout(&self) -> (usize, usize) {
        self.layout
    }

    fn process(
        &mut self,
        mut buffers: Buffers<Self::Sample>,
        cluster_idx: usize,
        _params: &dyn Parameters<Self::Sample>,
    ) -> <Self::Sample as SimdFloat>::Mask {
        let mut mask = <Self::Sample as SimdFloat>::Mask::splat(true);

        for task in &self.schedule {
            let handle = buffers.append(self.buffers.as_mut());

            match task {
                ProcessTask::Sum {
                    left_input,
                    right_input,
                    output,
                } => {
                    let l = handle.get_input_shared(*left_input).unwrap();
                    let r = handle.get_input_shared(*right_input).unwrap();
                    let output = handle.get_output_shared(*output).unwrap();

                    for ((l, r), output) in l.iter().zip(r).zip(output) {
                        output.set(l.get() + r.get())
                    }
                }

                ProcessTask::CopyToMasterOutput { input, outputs } => {
                    let input = handle.get_input_shared(*input).unwrap();

                    outputs
                        .iter()
                        .copied()
                        .map(OutputBufferIndex::Master)
                        .for_each(|index| {
                            let output = handle.get_output_shared(index).unwrap();
                            for (o, i) in output.iter().zip(input) {
                                o.set(i.get())
                            }
                        })
                }

                ProcessTask::Process {
                    index,
                    inputs,
                    outputs,
                } => {
                    let bufs = handle.with_indices(inputs, outputs);
                    mask &= self
                        .processors
                        .get_mut(*index)
                        .and_then(Option::as_mut)
                        .unwrap()
                        .process(bufs, cluster_idx, &());
                }
                ProcessTask::Delay {} => todo!(),
            }
        }

        mask
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) -> usize {
        self.buffers
            .iter_mut()
            .for_each(|buf| *buf = new_vfloat_buffer(max_buffer_size));

        self.processors().for_each(|proc| {
            proc.initialize(sr, max_buffer_size, max_num_clusters);
        });

        0
    }

    fn reset(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        _params: &dyn Parameters<Self::Sample>,
    ) {
        self.processors()
            .for_each(|proc| proc.reset(cluster_idx, voice_mask.clone(), &()))
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.processors().for_each(|proc| proc.move_state(from, to))
    }

    fn set_voice_notes(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
        note: <Self::Sample as SimdFloat>::Bits,
    ) {
        self.processors().for_each(|proc| {
            proc.set_voice_notes(cluster_idx, voice_mask.clone(), velocity, note.clone())
        })
    }

    fn deactivate_voices(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
    ) {
        self.processors()
            .for_each(|proc| proc.deactivate_voices(cluster_idx, voice_mask.clone(), velocity))
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
        params: &dyn Parameters<Self::Sample>,
    ) -> <Self::Sample as SimdFloat>::Mask {
        self.as_mut().process(buffers, cluster_idx, params)
    }

    fn initialize(&mut self, sr: f32, max_buffer_size: usize, max_num_clusters: usize) -> usize {
        self.as_mut()
            .initialize(sr, max_buffer_size, max_num_clusters)
    }

    fn reset(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        params: &dyn Parameters<Self::Sample>,
    ) {
        self.as_mut().reset(cluster_idx, voice_mask, params);
    }

    fn move_state(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.as_mut().move_state(from, to);
    }

    fn set_voice_notes(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
        note: <Self::Sample as SimdFloat>::Bits,
    ) {
        self.as_mut()
            .set_voice_notes(cluster_idx, voice_mask, velocity, note);
    }

    fn deactivate_voices(
        &mut self,
        cluster_idx: usize,
        voice_mask: <Self::Sample as SimdFloat>::Mask,
        velocity: Self::Sample,
    ) {
        self.as_mut()
            .deactivate_voices(cluster_idx, voice_mask, velocity)
    }
}
