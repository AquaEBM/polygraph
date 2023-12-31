use core::{cell::Cell, ops::Add};

pub type OwnedBuffer<T> = Box<[Cell<T>]>;

#[derive(Clone, Copy, Default)]
pub struct InputBuffer<'a, T>(&'a [Cell<T>]);

#[derive(Clone, Copy)]
pub struct Input<'a, T>(&'a Cell<T>);

impl<'a, T> InputBuffer<'a, T> {
    pub fn iter(self) -> impl Iterator<Item = Input<'a, T>> {
        self.0.iter().map(Input)
    }

    pub const fn len(self) -> usize {
        self.0.len()
    }
}

impl<'a, T: Copy> Input<'a, T> {
    pub fn get(self) -> T {
        self.0.get()
    }
}

#[derive(Clone, Copy, Default)]
pub struct OutputBuffer<'a, T>(&'a [Cell<T>]);

#[derive(Clone, Copy)]
pub struct Output<'a, T>(&'a Cell<T>);

impl<'a, T> OutputBuffer<'a, T> {
    pub fn iter(self) -> impl Iterator<Item = Output<'a, T>> {
        self.0.iter().map(Output)
    }

    pub const fn len(self) -> usize {
        self.0.len()
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

#[derive(Clone, Copy, Default)]
pub struct BufferHandle<'a, T> {
    parent: Option<&'a Buffers<'a, T>>,
    buffers: &'a [OwnedBuffer<T>],
}

impl<'a, T> BufferHandle<'a, T> {
    pub const fn parented(buffers: &'a [OwnedBuffer<T>], parent: &'a Buffers<'a, T>) -> Self {
        Self {
            parent: Some(parent),
            buffers,
        }
    }

    pub const fn toplevel(buffers: &'a [OwnedBuffer<T>]) -> Self {
        Self {
            parent: None,
            buffers,
        }
    }

    pub fn get_output_buffer(&self, buf_index: OutputBufferIndex) -> Option<OutputBuffer<T>> {
        match buf_index {
            OutputBufferIndex::Global(i) => self.parent.unwrap().get_output(i),
            OutputBufferIndex::Intermediate(i) => Some(OutputBuffer(self.buffers[i].as_ref()))
        }
    }

    pub fn get_input_buffer(&self, buf_index: BufferIndex) -> Option<InputBuffer<T>> {
        match buf_index {
            BufferIndex::GlobalInput(i) => self.parent.unwrap().get_input(i),
            BufferIndex::Output(buf_index) => self
                .get_output_buffer(buf_index)
                .map(OutputBuffer::as_input),
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
pub struct Buffers<'a, T> {
    buffer_handle: BufferHandle<'a, T>,
    inputs: &'a [Option<BufferIndex>],
    outputs: &'a [Option<OutputBufferIndex>],
}

impl<'a, T> Buffers<'a, T> {
    pub const fn with_handle(buffer_handle: BufferHandle<'a, T>) -> Self {
        Self {
            buffer_handle,
            inputs: &[],
            outputs: &[],
        }
    }

    pub fn set_inputs(&mut self, inputs: &'a [Option<BufferIndex>]) {
        self.inputs = inputs;
    }

    pub fn set_outputs(&mut self, outputs: &'a [Option<OutputBufferIndex>]) {
        self.outputs = outputs;
    }

    pub fn set_handle(&mut self, buffer_handle: BufferHandle<'a, T>) {
        self.buffer_handle = buffer_handle;
    }

    pub const fn with_handle_and_io(
        buffer_handle: BufferHandle<'a, T>,
        inputs: &'a [Option<BufferIndex>],
        outputs: &'a [Option<OutputBufferIndex>],
    ) -> Self {
        Self {
            buffer_handle,
            inputs,
            outputs,
        }
    }

    pub fn get_input(&self, index: usize) -> Option<InputBuffer<T>> {
        self.inputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index.and_then(|buf_index| self.buffer_handle.get_input_buffer(buf_index))
        })
    }

    pub fn get_output(&self, index: usize) -> Option<OutputBuffer<T>> {
        self.outputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index.and_then(|buf_index| self.buffer_handle.get_output_buffer(buf_index))
        })
    }
}
