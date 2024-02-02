use core::{cell::Cell, mem::transmute, num::NonZeroUsize, ops::Add};

pub type OwnedBuffer<T> = Box<Cell<[T]>>;

/// # Safety
/// T must be safely zeroable
pub(crate) unsafe fn new_owned_buffer<T>(len: usize) -> OwnedBuffer<T> {
    // SAFETY: T is zeroable, and Cell<T> has the same layout as T
    transmute(Box::<[T]>::new_zeroed_slice(len).assume_init())
}

#[derive(Clone, Copy, Default)]
pub struct InputBuffer<'a, T>(&'a [Cell<T>]);

#[derive(Clone, Copy)]
pub struct Input<'a, T>(&'a Cell<T>);

impl<'a, T: Copy> Input<'a, T> {
    pub fn get(self) -> T {
        self.0.get()
    }
}

impl<'a, T> InputBuffer<'a, T> {
    pub fn iter(self) -> impl Iterator<Item = Input<'a, T>> {
        self.0.iter().map(Input)
    }

    pub const fn len(self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

impl<'a, T> From<&'a [Cell<T>]> for InputBuffer<'a, T> {
    fn from(value: &'a [Cell<T>]) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Default)]
pub struct OutputBuffer<'a, T>(&'a [Cell<T>]);

#[derive(Clone, Copy)]
pub struct Output<'a, T>(&'a Cell<T>);

impl<'a, T> Output<'a, T> {
    pub fn get(self) -> T
    where
        T: Copy,
    {
        self.0.get()
    }

    pub fn set(self, value: T) {
        self.0.set(value)
    }
}

impl<'a, T> OutputBuffer<'a, T> {
    pub fn iter(self) -> impl Iterator<Item = Output<'a, T>> {
        self.0.iter().map(Output)
    }

    pub const fn len(self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub const fn as_input(self) -> InputBuffer<'a, T> {
        InputBuffer(self.0)
    }

    pub fn add<U>(self, left: InputBuffer<'a, U>, right: InputBuffer<'a, U>)
    where
        U: Add<Output = T> + Copy,
    {
        for (output, (left, right)) in self.iter().zip(left.iter().zip(right.iter())) {
            output.set(left.get() + right.get())
        }
    }

    pub fn copy(self, other: InputBuffer<'a, T>)
    where
        T: Copy,
    {
        for (output, input) in self.iter().zip(other.iter()) {
            output.set(input.get())
        }
    }
}

#[derive(Default)]
pub(crate) struct BufferHandle<'a, T> {
    parent: Option<&'a BufferIndices<'a, T>>,
    buffers: &'a [OwnedBuffer<T>],
}

impl<'a, T> BufferHandle<'a, T> {
    pub(crate) fn parented(
        buffers: &'a [OwnedBuffer<T>],
        parent: &'a BufferIndices<'a, T>,
    ) -> Self {
        Self {
            parent: Some(parent),
            buffers,
        }
    }

    pub(crate) fn toplevel(buffers: &'a [OwnedBuffer<T>]) -> Self {
        Self {
            parent: None,
            buffers,
        }
    }

    pub(crate) fn get_output_buffer(
        &'a self,
        buf_index: OutputBufferIndex,
        start: usize,
        len: usize,
    ) -> Option<OutputBuffer<'a, T>> {
        match buf_index {
            OutputBufferIndex::Global(i) => self.parent.as_ref().unwrap().get_output(i, start, len),
            OutputBufferIndex::Intermediate(i) => Some(OutputBuffer(
                &self.buffers[i].as_slice_of_cells()[start..start + len],
            )),
        }
    }

    pub(crate) fn get_input_buffer(
        &'a self,
        buf_index: BufferIndex,
        start: usize,
        len: usize,
    ) -> Option<InputBuffer<'a, T>> {
        match buf_index {
            BufferIndex::GlobalInput(i) => self.parent.as_ref().unwrap().get_input(i, start, len),
            BufferIndex::Output(buf) => self
                .get_output_buffer(buf, start, len)
                .map(OutputBuffer::as_input),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub(crate) enum OutputBufferIndex {
    Global(usize),
    Intermediate(usize),
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub(crate) enum BufferIndex {
    GlobalInput(usize),
    Output(OutputBufferIndex),
}

pub(crate) struct BufferIndices<'a, T> {
    handle: BufferHandle<'a, T>,
    inputs: &'a [Option<BufferIndex>],
    outputs: &'a [Option<OutputBufferIndex>],
}

impl<'a, T> BufferIndices<'a, T> {
    pub(crate) fn with_handle(buffer_handle: BufferHandle<'a, T>) -> Self {
        Self::with_handle_and_io(buffer_handle, &[], &[])
    }

    pub(crate) fn set_inputs(&mut self, inputs: &'a [Option<BufferIndex>]) {
        self.inputs = inputs;
    }

    pub(crate) fn set_outputs(&mut self, outputs: &'a [Option<OutputBufferIndex>]) {
        self.outputs = outputs;
    }

    pub(crate) fn set_handle(&mut self, buffer_handle: BufferHandle<'a, T>) {
        self.handle = buffer_handle;
    }

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

    pub(crate) fn get_input(
        &'a self,
        index: usize,
        start: usize,
        len: usize,
    ) -> Option<InputBuffer<'a, T>> {
        self.inputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index
                .and_then(|buf_index| self.handle.get_input_buffer(buf_index, start, len))
        })
    }

    pub(crate) fn get_output(
        &'a self,
        index: usize,
        start: usize,
        len: usize,
    ) -> Option<OutputBuffer<'a, T>> {
        self.outputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index
                .and_then(|buf_index| self.handle.get_output_buffer(buf_index, start, len))
        })
    }
}

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

    pub fn get_input(&'a self, index: usize) -> Option<InputBuffer<'a, T>> {
        self.indices.get_input(index, self.start, self.len.get())
    }

    pub fn get_output(&'a self, index: usize) -> Option<OutputBuffer<'a, T>> {
        self.indices.get_output(index, self.start, self.len.get())
    }
}
