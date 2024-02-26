use core::{
    cell::Cell,
    mem::{self, transmute},
    num::NonZeroUsize,
};

use simd_util::{
    simd::{Simd, SimdElement},
    split_stereo_cell, FLOATS_PER_VECTOR, STEREO_VOICES_PER_VECTOR,
};

pub type OwnedBuffer<T> = Box<Cell<[T]>>;

/// # Safety
/// T must be safely zeroable
pub(crate) unsafe fn new_owned_buffer<T>(len: usize) -> OwnedBuffer<T> {
    // SAFETY: T is zeroable, and Cell<T> has the same layout as T
    transmute(Box::<[T]>::new_zeroed_slice(len).assume_init())
}

pub struct ReadOnly<T: ?Sized>(Cell<T>);

impl<T: ?Sized> ReadOnly<T> {
    #[inline]
    pub fn from_cell(cell: &Cell<T>) -> &Self {
        unsafe { mem::transmute(cell) }
    }
}

impl<T> ReadOnly<[T]> {
    #[inline]
    pub fn transpose(&self) -> &[ReadOnly<T>] {
        unsafe { mem::transmute(self) }
    }
}

impl<T, const N: usize> ReadOnly<[T; N]> {
    #[inline]
    pub fn transpose(&self) -> &[ReadOnly<T>; N] {
        unsafe { mem::transmute(self) }
    }
}

impl<T> ReadOnly<T> {
    #[inline]
    pub fn from_slice(cell_slice: &[Cell<T>]) -> &[Self] {
        unsafe { mem::transmute(cell_slice) }
    }

    #[inline]
    pub fn get(&self) -> T
    where
        T: Copy,
    {
        self.0.get()
    }
}

impl<T: SimdElement> ReadOnly<Simd<T, FLOATS_PER_VECTOR>> {
    #[inline]
    pub fn split_stereo(&self) -> &[ReadOnly<Simd<T, 2>>; STEREO_VOICES_PER_VECTOR] {
        ReadOnly::from_cell(split_stereo_cell(&self.0)).transpose()
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BufferHandle<'a, T> {
    parent: Option<&'a BufferIndices<'a, T>>,
    buffers: &'a [OwnedBuffer<T>],
}

impl<'a, T> BufferHandle<'a, T> {
    #[inline]
    pub(crate) fn parented(
        buffers: &'a [OwnedBuffer<T>],
        parent: &'a BufferIndices<'a, T>,
    ) -> Self {
        Self {
            parent: Some(parent),
            buffers,
        }
    }

    #[inline]
    pub(crate) fn toplevel(buffers: &'a [OwnedBuffer<T>]) -> Self {
        Self {
            parent: None,
            buffers,
        }
    }

    #[inline]
    pub(crate) fn get_output_buffer(
        &'a self,
        buf_index: OutputBufferIndex,
        start: usize,
        len: usize,
    ) -> Option<&'a [Cell<T>]> {
        match buf_index {
            OutputBufferIndex::Global(i) => self.parent.as_ref().unwrap().get_output(i, start, len),
            OutputBufferIndex::Intermediate(i) => {
                Some(&self.buffers[i].as_slice_of_cells()[start..start + len])
            }
        }
    }

    #[inline]
    pub(crate) fn get_input_buffer(
        &'a self,
        buf_index: BufferIndex,
        start: usize,
        len: usize,
    ) -> Option<&'a [ReadOnly<T>]> {
        match buf_index {
            BufferIndex::GlobalInput(i) => self.parent.as_ref().unwrap().get_input(i, start, len),
            BufferIndex::Output(buf) => self
                .get_output_buffer(buf, start, len)
                .map(ReadOnly::from_slice),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum OutputBufferIndex {
    Global(usize),
    Intermediate(usize),
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum BufferIndex {
    GlobalInput(usize),
    Output(OutputBufferIndex),
}

#[derive(Clone, Copy, Default)]
pub struct BufferIndices<'a, T> {
    handle: BufferHandle<'a, T>,
    inputs: &'a [Option<BufferIndex>],
    outputs: &'a [Option<OutputBufferIndex>],
}

impl<'a, T> BufferIndices<'a, T> {
    #[inline]
    pub(crate) fn with_handle(buffer_handle: BufferHandle<'a, T>) -> Self {
        Self::with_handle_and_io(buffer_handle, &[], &[])
    }

    #[inline]
    pub(crate) fn set_inputs(&mut self, inputs: &'a [Option<BufferIndex>]) {
        self.inputs = inputs;
    }

    #[inline]
    pub(crate) fn set_outputs(&mut self, outputs: &'a [Option<OutputBufferIndex>]) {
        self.outputs = outputs;
    }

    #[inline]
    pub(crate) fn set_handle(&mut self, buffer_handle: BufferHandle<'a, T>) {
        self.handle = buffer_handle;
    }

    #[inline]
    pub(crate) fn with_handle_and_io(
        handle: BufferHandle<'a, T>,
        inputs: &'a [Option<BufferIndex>],
        outputs: &'a [Option<OutputBufferIndex>],
    ) -> Self {
        Self {
            handle,
            inputs,
            outputs,
        }
    }

    #[inline]
    pub(crate) fn get_input(
        &'a self,
        index: usize,
        start: usize,
        len: usize,
    ) -> Option<&'a [ReadOnly<T>]> {
        self.inputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index
                .and_then(|buf_index| self.handle.get_input_buffer(buf_index, start, len))
        })
    }

    #[inline]
    pub(crate) fn get_output(
        &'a self,
        index: usize,
        start: usize,
        len: usize,
    ) -> Option<&'a [Cell<T>]> {
        self.outputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index
                .and_then(|buf_index| self.handle.get_output_buffer(buf_index, start, len))
        })
    }
}

#[derive(Clone, Copy)]
pub struct Buffers<'a, T> {
    start: usize,
    len: NonZeroUsize,
    indices: BufferIndices<'a, T>,
}

impl<'a, T> Buffers<'a, T> {
    pub(crate) fn new(
        start: usize,
        len: NonZeroUsize,
        handle: BufferHandle<'a, T>,
        inputs: &'a [Option<BufferIndex>],
        outputs: &'a [Option<OutputBufferIndex>],
    ) -> Self {
        Self {
            start,
            len,
            indices: BufferIndices::with_handle_and_io(handle, inputs, outputs),
        }
    }

    pub(crate) fn start(&self) -> usize {
        self.start
    }

    pub fn buffer_size(&self) -> NonZeroUsize {
        self.len
    }

    pub(crate) fn indices(&'a self) -> &'a BufferIndices<'a, T> {
        &self.indices
    }

    pub fn get_input(&'a self, index: usize) -> Option<&'a [ReadOnly<T>]> {
        self.indices.get_input(index, self.start, self.len.get())
    }

    pub fn get_output(&'a self, index: usize) -> Option<&'a [Cell<T>]> {
        self.indices.get_output(index, self.start, self.len.get())
    }
}
