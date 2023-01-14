/// Safe abstraction for taking ownership of values behind a mutable reference.
/// This is useful because the [`hyper::service::Service`] trait that we use
/// to handle connections provides us with `&mut self` and we have to return
/// a [`std::future::Future`] that owns its data, but we can't move out of
/// `self`, which is behind a mutable reference :)
pub(crate) struct Movable<T> {
    inner: Option<T>,
}

/// Errors than can happen while moving out of the inner value.
#[derive(Debug)]
pub(crate) enum Error {
    AlreadyTaken,
}

impl<T> Movable<T> {
    /// Creates a new [`Movable`] that holds the given `value`.
    pub fn new(value: T) -> Self {
        Self { inner: Some(value) }
    }

    /// Moves out of the inner value and returns it as owned. If the value
    /// has already been taken an error is returned instead.
    pub fn take(&mut self) -> Result<T, Error> {
        self.inner.take().ok_or(Error::AlreadyTaken)
    }
}
