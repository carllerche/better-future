use futures::{Async, Poll, Stream};

use {Watch, WatchError};

/// Maps borrowed references to `T` into an `Item`.
pub trait Then<T> {
    /// The output type.
    type Output;

    /// What you get when Map fails.
    type Error;

    /// Produces a new Output value.
    fn then(&mut self, t: Result<&T, WatchError>) -> Result<Self::Output, Self::Error>;
}

/// Each time the underlying `Watch<T>` is updated, the stream maps over the most-recent
/// value.
#[derive(Debug)]
pub struct ThenStream<T, M: Then<T>> {
    watch: Watch<T>,
    then: M,
}

// ==== impl ThenStream ====

impl<T, M: Then<T>> ThenStream<T, M> {
    pub(crate) fn new(watch: Watch<T>, then: M) -> Self {
        Self { watch, then }
    }
}

impl<T, M: Then<T>> Stream for ThenStream<T, M> {
    type Item = <M as Then<T>>::Output;
    type Error = <M as Then<T>>::Error;

    fn poll(&mut self) -> Poll<Option<M::Output>, Self::Error> {
        let result = match self.watch.poll() {
            Ok(Async::Ready(Some(()))) => {
                self.then.then(Ok(&*self.watch.borrow()))
            }
            Err(e) => {
                self.then.then(Err(e))
            }
            Ok(Async::NotReady) => {
                return Ok(Async::NotReady);
            }
            Ok(Async::Ready(None)) => {
                return Ok(Async::Ready(None));
            }
        };

        result.map(Some).map(Async::Ready)
    }
}

impl<T, M: Clone + Then<T>> Clone for ThenStream<T, M> {
    fn clone(&self) -> Self {
        Self::new(self.watch.clone(), self.then.clone())
    }
}

// ==== impl Then ====

impl<T, O, E, F> Then<T> for F
where
    for<'t> F: FnMut(Result<&T, WatchError>) -> Result<O, E>,
{
    type Output = O;
    type Error = E;

    fn then(&mut self, t: Result<&T, WatchError>) -> Result<O, E> {
        (self)(t)
    }
}
