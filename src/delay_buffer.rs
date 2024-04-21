use core::{iter, mem, num::NonZeroUsize};

#[derive(Clone, Debug, Default)]
pub struct FixedDelayBuffer<T> {
    buf: Box<[T]>,
    current: usize,
}

impl<T> FixedDelayBuffer<T> {
    pub fn new(num_samples: NonZeroUsize) -> Self
    where
        T: Default,
    {
        Self {
            buf: iter::repeat_with(T::default)
                .take(num_samples.get())
                .collect(),
            ..Default::default()
        }
    }

    pub fn delay(&mut self, buf: &mut [T]) {
        // TODO: optimize
        for sample in buf {
            mem::swap(&mut self.buf[self.current], sample);
            self.current = (self.current + 1) % self.buf.len();
        }
    }
}
