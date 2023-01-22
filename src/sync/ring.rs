use std::sync::atomic::{AtomicUsize, Ordering};

/// Provides circular read-only access to the elements of an array. This is used
/// for schedulers, since some of them can pre-compute a complete cycle and then
/// return elements from that cycle when needed. For example, a WRR scheduler
/// for 3 servers A, B and C with weights 1, 3 and 2 might compute the next
/// cycle: `[A, B, B, B, C, C]`. When the scheduler is asked for the next server
/// that should handle a request (see [`crate::sched`]), it only needs to return
/// a value from the cycle array. When it returns the last value, it can start
/// again from the beginning because all cycles are equal. The only caveat is
/// that the calculation of the next index has to be atomic since multiple
/// threads can process requests at the same time.
#[derive(Debug)]
pub(crate) struct Ring<T> {
    /// All the elements in this ring.
    values: Vec<T>,

    /// Index of the next value that we should return.
    next: AtomicUsize,
}

impl<T> Ring<T> {
    /// Creates a new [`Ring`]. The first value returned when calling one of
    /// the getter functions is going to be located at index 0 in `values` vec.
    /// Subsequent calls to any getter will return the value at the next index
    /// until the last one is reached, after that it starts again from the
    /// beginning.
    pub fn new(values: Vec<T>) -> Self {
        Self {
            values,
            next: AtomicUsize::new(0),
        }
    }
}

impl<T> Ring<T> {
    /// Computes the index of the next value that has to be returned.
    #[inline]
    fn next_index(&self) -> usize {
        if self.values.len() == 1 {
            0
        } else {
            self.next.fetch_add(1, Ordering::Relaxed) % self.values.len()
        }
    }

    /// Returns a reference to the next value in the ring.
    #[inline]
    pub fn next_as_ref(&self) -> &T {
        &self.values[self.next_index()]
    }
}

impl<T: Copy> Ring<T> {
    /// Returns the next value in the ring by making a copy.
    #[inline]
    pub fn next_as_owned(&self) -> T {
        *self.next_as_ref()
    }
}

impl<T: Clone> Ring<T> {
    /// Returns the next value in the ring by cloning it.
    #[allow(dead_code)]
    #[inline]
    pub fn next_as_cloned(&self) -> T {
        self.next_as_ref().clone()
    }
}
