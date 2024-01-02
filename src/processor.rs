use crate::*;
use core::{iter, mem, num::NonZeroUsize, ops::Add};

pub struct AudioGraphProcessor<T, const N: usize>
where
    LaneCount<N>: SupportedLaneCount,
    T: SimdElement,
{
    processors: Vec<Option<Box<dyn Processor<T, N>>>>,
    schedule: Vec<ProcessTask>,
    buffers: Box<[Buffer<Simd<T, N>>]>,
}

impl<T, const N: usize> Default for AudioGraphProcessor<T, N>
where
    LaneCount<N>: SupportedLaneCount,
    T: SimdElement,
{
    fn default() -> Self {
        Self {
            processors: Default::default(),
            schedule: Default::default(),
            buffers: Default::default(),
        }
    }
}

impl<T, const N: usize> AudioGraphProcessor<T, N>
where
    LaneCount<N>: SupportedLaneCount,
    T: SimdElement,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_schedule(&mut self, schedule: Vec<ProcessTask>) -> Vec<ProcessTask> {
        mem::replace(&mut self.schedule, schedule)
    }

    pub fn replace_buffers(
        &mut self,
        buffers: Box<[Buffer<Simd<T, N>>]>,
    ) -> Box<[Buffer<Simd<T, N>>]> {
        mem::replace(&mut self.buffers, buffers)
    }

    pub fn insert_processor(&mut self, processor: Box<dyn Processor<T, N>>) -> usize {
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

    pub fn remove_processor(&mut self, index: usize) -> Option<Box<dyn Processor<T, N>>> {
        self.processors
            .get_mut(index)
            .and_then(|maybe_proc| maybe_proc.take())
    }

    pub fn schedule_for(&mut self, graph: &AudioGraph, buffer_size: usize) {
        let (schedule, num_buffers) = graph.compile();

        self.schedule = schedule;

        self.buffers = iter::repeat(
            // SAFETY: for all T: SimdElement, T is safely zeroable, thus Simd<T, N> is too
            unsafe { Box::new_zeroed_slice(buffer_size).assume_init() },
        )
        .take(num_buffers)
        .collect()
    }
}

impl<T, const N: usize> Processor<T, N> for AudioGraphProcessor<T, N>
where
    LaneCount<N>: SupportedLaneCount,
    T: SimdElement,
    Simd<T, N>: Add<Output = Simd<T, N>>,
{
    fn process(
        &mut self,
        buffers: Buffers<Simd<T, N>>,
        cluster_idx: usize,
        params_changed: Option<NonZeroUsize>,
    ) {
        let buffer_handle = BufferHandle::parented(self.buffers.as_ref(), &buffers);

        for task in &self.schedule {
            match task {
                ProcessTask::Add {
                    left_input,
                    right_input,
                    output,
                } => buffer_handle.get_output_buffer(*output).unwrap().add(
                    buffer_handle.get_input_buffer(*left_input).unwrap(),
                    buffer_handle.get_input_buffer(*right_input).unwrap(),
                ),

                ProcessTask::Copy { input, outputs } => {
                    let input = buffer_handle.get_input_buffer(*input).unwrap();

                    for &output in outputs.iter() {
                        for (input, output) in input
                            .iter()
                            .zip(buffer_handle.get_output_buffer(output).unwrap().iter())
                        {
                            output.set(input.get())
                        }
                    }
                }

                ProcessTask::Process {
                    index,
                    inputs,
                    outputs,
                } => {
                    let buffers = Buffers::with_handle_and_io(
                        &buffer_handle,
                        inputs.as_ref(),
                        outputs.as_ref(),
                    );

                    self.processors[*index].as_mut().unwrap().process(
                        buffers,
                        cluster_idx,
                        params_changed,
                    );
                }
            }
        }
    }

    fn set_max_polyphony(&mut self, num_clusters: usize) {
        self.processors
            .iter_mut()
            .filter_map(Option::as_deref_mut)
            .for_each(|proc| proc.set_max_polyphony(num_clusters))
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
