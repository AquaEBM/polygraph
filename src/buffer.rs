use core::{
    cell::Cell,
    mem::{self, transmute},
    num::NonZeroUsize,
};

use simd_util::{
    simd::{Simd, SimdElement},
    split_stereo_cell, FLOATS_PER_VECTOR, STEREO_VOICES_PER_VECTOR,
};

/// This is a wrapper around a `Cell<T>` that only allows for reading the contained value
#[repr(transparent)]
pub struct ReadOnly<T: ?Sized>(Cell<T>);

// SAFETY (for all the `mem::transmute`s used in the implementations of `ReadOnly<T>`): read the above doc comment

impl<T: ?Sized> ReadOnly<T> {
    #[inline]
    pub fn from_cell_ref(cell: &Cell<T>) -> &Self {
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
    pub fn from_cell(cell: Cell<T>) -> Self {
        Self(cell)
    }

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
    pub fn split_stereo(&self) -> &ReadOnly<[Simd<T, 2>; STEREO_VOICES_PER_VECTOR]> {
        ReadOnly::from_cell_ref(split_stereo_cell(&self.0))
    }
}

impl<T: SimdElement> ReadOnly<[Simd<T, FLOATS_PER_VECTOR>]> {
    #[inline]
    pub fn split_stereo_slice(&self) -> &[[ReadOnly<Simd<T, 2>>; STEREO_VOICES_PER_VECTOR]] {
        unsafe { mem::transmute(self) }
    }
}

pub type OwnedBuffer<T> = Box<Cell<[T]>>;

/// # Safety
/// T must be safely zeroable
#[inline]
pub(crate) unsafe fn new_zeroed_owned_buffer<T>(len: usize) -> OwnedBuffer<T> {
    // SAFETY: T is zeroable, and Cell<T> has the same layout as T
    transmute(Box::<[T]>::new_zeroed_slice(len).assume_init())
}

// The following structs describe a linked list-like interface in order to allow
// audio graph nodes (and potentially others nested in them) to (re)use buffers
// from their callers as master/global inputs/outputs
//
// the tricks described in this discussion are used:
// https://users.rust-lang.org/t/safe-interface-for-a-singly-linked-list-of-mutable-references/107401

pub struct BufferNode<'a, T> {
    // the most notable trick here is the usage of a trait object to represent a nested `BufferNode<'_, T>`,
    // since trait objects (dyn Trait + 'a) are covariant over their inner lifetime(s) ('a) this pattern
    // is usable, and very powerful, in spite of &'a mut T being invariant over T.
    parent: Option<&'a mut dyn BufferHandleInner<T>>,
    buffers: &'a mut [OwnedBuffer<T>],
}

impl<'a, T> Default for BufferNode<'a, T> {
    #[inline]
    fn default() -> Self {
        Self {
            parent: Default::default(),
            buffers: Default::default(),
        }
    }
}

impl<'a, T> BufferNode<'a, T> {
    #[inline]
    pub fn toplevel(buffers: &'a mut [OwnedBuffer<T>]) -> Self {
        Self {
            parent: None,
            buffers,
        }
    }

    #[inline]
    pub fn with_indices(
        self,
        inputs: &'a [Option<BufferIndex>],
        outputs: &'a [Option<OutputBufferIndex>],
    ) -> BufferHandle<'a, T> {
        BufferHandle {
            node: self,
            inputs,
            outputs,
        }
    }

    #[inline]
    pub fn get_input(&mut self, buf_index: BufferIndex) -> Option<&[T]> {
        match buf_index {
            BufferIndex::GlobalInput(i) => self.parent.as_mut().unwrap().get_input(i),
            BufferIndex::Output(buf) => self.get_output(buf).map(|buf| &*buf),
        }
    }

    #[inline]
    pub fn get_input_shared(&self, buf_index: BufferIndex) -> Option<&[ReadOnly<T>]> {
        match buf_index {
            BufferIndex::GlobalInput(i) => self.parent.as_ref().unwrap().get_input_shared(i),
            BufferIndex::Output(buf) => self.get_output_shared(buf).map(ReadOnly::from_slice),
        }
    }

    #[inline]
    pub fn get_output(&mut self, buf_index: OutputBufferIndex) -> Option<&mut [T]> {
        match buf_index {
            OutputBufferIndex::Global(i) => self.parent.as_mut().unwrap().get_output(i),
            OutputBufferIndex::Intermediate(i) => Some(Cell::get_mut(&mut self.buffers[i])),
        }
    }

    #[inline]
    pub fn get_output_shared(&self, buf_index: OutputBufferIndex) -> Option<&[Cell<T>]> {
        match buf_index {
            OutputBufferIndex::Global(i) => self.parent.as_ref().unwrap().get_output_shared(i),
            OutputBufferIndex::Intermediate(i) => Some(&self.buffers[i].as_slice_of_cells()),
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

pub trait BufferHandleInner<T> {
    fn get_input(&mut self, index: usize) -> Option<&[T]>;

    fn get_input_shared(&self, index: usize) -> Option<&[ReadOnly<T>]>;

    fn get_output(&mut self, index: usize) -> Option<&mut [T]>;

    fn get_output_shared(&self, index: usize) -> Option<&[Cell<T>]>;
}

pub struct BufferHandle<'a, T> {
    node: BufferNode<'a, T>,
    inputs: &'a [Option<BufferIndex>],
    outputs: &'a [Option<OutputBufferIndex>],
}

impl<'a, T> Default for BufferHandle<'a, T> {
    #[inline]
    fn default() -> Self {
        Self {
            node: Default::default(),
            inputs: Default::default(),
            outputs: Default::default(),
        }
    }
}

impl<'a, T> BufferHandle<'a, T> {
    #[inline]
    pub fn append<'b>(&'b mut self, buffers: &'b mut [OwnedBuffer<T>]) -> BufferNode<'b, T> {
        BufferNode {
            parent: Some(self),
            buffers,
        }
    }

    #[inline]
    pub fn with_buffer_pos(self, start: usize, len: NonZeroUsize) -> Buffers<'a, T> {
        Buffers {
            handle: self,
            start,
            len,
        }
    }
}

impl<'a, T> BufferHandleInner<T> for BufferHandle<'a, T> {
    #[inline]
    fn get_input(&mut self, index: usize) -> Option<&[T]> {
        self.inputs.get(index).and_then(|maybe_index| {
            maybe_index.and_then(|buf_index| self.node.get_input(buf_index))
        })
    }

    #[inline]
    fn get_input_shared(&self, index: usize) -> Option<&[ReadOnly<T>]> {
        self.inputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index.and_then(|buf_index| self.node.get_input_shared(buf_index))
        })
    }

    #[inline]
    fn get_output(&mut self, index: usize) -> Option<&mut [T]> {
        self.outputs.get(index).and_then(|maybe_index| {
            maybe_index.and_then(|buf_index| self.node.get_output(buf_index))
        })
    }

    #[inline]
    fn get_output_shared(&self, index: usize) -> Option<&[Cell<T>]> {
        self.outputs.get(index).and_then(|maybe_buf_index| {
            maybe_buf_index.and_then(|buf_index| self.node.get_output_shared(buf_index))
        })
    }
}

pub struct Buffers<'a, T> {
    start: usize,
    len: NonZeroUsize,
    handle: BufferHandle<'a, T>,
}

impl<'a, T> Buffers<'a, T> {
    #[inline]
    pub(crate) fn start(&self) -> usize {
        self.start
    }

    #[inline]
    pub fn buffer_size(&self) -> NonZeroUsize {
        self.len
    }

    #[inline]
    pub fn handle(&mut self) -> &mut BufferHandle<'a, T> {
        &mut self.handle
    }

    #[inline]
    pub fn get_input(&mut self, index: usize) -> Option<&[T]> {
        self.handle
            .get_input(index)
            .map(|buf| &buf[self.start..][..self.len.get()])
    }

    #[inline]
    pub fn get_input_shared(&self, index: usize) -> Option<&[ReadOnly<T>]> {
        self.handle
            .get_input_shared(index)
            .map(|buf| &buf[self.start..][..self.len.get()])
    }

    #[inline]
    pub fn get_output(&mut self, index: usize) -> Option<&mut [T]> {
        self.handle
            .get_output(index)
            .map(|buf| &mut buf[self.start..][..self.len.get()])
    }

    #[inline]
    pub fn get_output_shared(&self, index: usize) -> Option<&[Cell<T>]> {
        self.handle
            .get_output_shared(index)
            .map(|buf| &buf[self.start..][..self.len.get()])
    }
}
