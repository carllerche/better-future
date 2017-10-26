//! Futures-aware borrow cell.
//!
//! Given the asynchronous nature of working with futures, managing borrows that
//! live across callbacks can be difficult. This is because lifetimes cannot be
//! moved into closures that are `'static` (i.e. most of the futures-rs
//! combinators).
//!
//! `Borrow` provides runtime checked borrowing, similar to `RefCell`, however
//! `Borrow` also provides `Future` task notifications when borrows are dropped.

extern crate futures;

use futures::{Poll, Async};
use futures::task::AtomicTask;

use std::{fmt, ops, thread};
use std::any::Any;
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

/// A mutable memory location with future-aware dynamically checked borrow
/// rules.
///
/// Safe borrowing of data across `Future` tasks requires that the data is
/// stored in stable memory. To do this, `Borrow` internally creates an `Arc` to
/// store the data.
///
/// See crate level documentation for more details.
pub struct Borrow<T> {
    /// The borrow state.
    ///
    /// The state is stored in an `Arc` in order to ensure that it does not move
    /// to a different memory location while it is being borrowed.
    inner: Arc<Inner<T>>,
}

/// Holds a borrowed value obtained from `Borrow`.
///
/// When this value is dropped, the borrow is released, notiying any pending
/// tasks.
pub struct BorrowGuard<T> {
    /// The borrowed ref. This could be a pointer to an inner field of the `T`
    /// stored by `Borrow`.
    value_ptr: *mut T,

    /// Borrowed state
    handle: BorrowHandle,
}

/// Error produced by a failed `poll_borrow` call.
#[derive(Debug)]
pub struct BorrowError {
    _priv: (),
}

/// Error produced by a failed `try_borrow` call.
#[derive(Debug)]
pub struct TryBorrowError {
    is_poisoned: bool,
}

struct Inner<T> {
    /// The value that can be valued
    value: UnsafeCell<T>,

    /// Borrow state
    state: State,
}

struct BorrowHandle {
    /// The borrow state
    state_ptr: *const State,

    /// Holds a handle to the Arc, which prevents it from being dropped.
    _inner: Arc<Any>,
}

struct State {
    /// Tracks if the value is currently borrowed or poisoned.
    borrowed: AtomicUsize,

    /// The task to notify once the borrow is released
    task: AtomicTask,
}

const UNUSED: usize = 0;
const BORROWED: usize = 1;
const POISONED: usize = 2;

// ===== impl Borrow =====

impl<T: 'static> Borrow<T> {
    /// Create a new `Borrow` containing `value`.
    pub fn new(value: T) -> Borrow<T> {
        Borrow {
            inner: Arc::new(Inner {
                value: UnsafeCell::new(value),
                state: State {
                    borrowed: AtomicUsize::new(UNUSED),
                    task: AtomicTask::new(),
                },
            }),
        }
    }

    /// Returns `true` if the value is not already borrowed.
    pub fn is_ready(&self) -> bool {
        match self.inner.state.borrowed.load(Acquire) {
            UNUSED => true,
            BORROWED => false,
            POISONED => true,
            _ => unreachable!(),
        }
    }

    /// Returns `Ready` when the value is not already borrowed.
    ///
    /// When `Ready` is returned, the next call to `poll_borrow` or `try_borrow`
    /// is guaranteed to succeed. When `NotReady` is returned, the current task
    /// will be notified once the outstanding borrow is released.
    pub fn poll_ready(&mut self) -> Poll<(), BorrowError> {
        self.inner.state.task.register();

        match self.inner.state.borrowed.load(Acquire) {
            UNUSED => Ok(Async::Ready(())),
            BORROWED => Ok(Async::NotReady),
            POISONED => Err(BorrowError::new()),
            _ => unreachable!(),
        }
    }

    /// Attempt to borrow the value, returning `NotReady` if it cannot be
    /// borrowed.
    pub fn poll_borrow(&mut self) -> Poll<BorrowGuard<T>, BorrowError> {
        self.inner.state.task.register();

        match self.inner.state.borrowed.compare_and_swap(UNUSED, BORROWED, Acquire) {
            UNUSED => {
                // Lock acquired, fall through
            }
            BORROWED => return Ok(Async::NotReady),
            POISONED => return Err(BorrowError::new()),
            _ => unreachable!(),
        }

        let value_ptr = self.inner.value.get();
        let handle = BorrowHandle {
            state_ptr: &self.inner.state as *const State,
            _inner: self.inner.clone() as Arc<Any>,
        };

        Ok(Async::Ready(BorrowGuard {
            value_ptr,
            handle,
        }))
    }

    /// Attempt to borrow the value, returning `Err` if it cannot be borrowed.
    pub fn try_borrow(&self) -> Result<BorrowGuard<T>, TryBorrowError> {
        match self.inner.state.borrowed.compare_and_swap(UNUSED, BORROWED, Acquire) {
            UNUSED => {
                // Lock acquired, fall through
            }
            BORROWED => return Err(TryBorrowError::new(false)),
            POISONED => return Err(TryBorrowError::new(true)),
            _ => unreachable!(),
        }

        let value_ptr = self.inner.value.get();
        let handle = BorrowHandle {
            state_ptr: &self.inner.state as *const State,
            _inner: self.inner.clone() as Arc<Any>,
        };

        Ok(BorrowGuard {
            value_ptr,
            handle,
        })
    }

    /// Make a new `BorrowGuard` for a component of the borrowed data.
    ///
    /// The `BorrowGuard` is already mutably borrowed, so this cannot fail.
    pub fn map<F, U>(mut r: BorrowGuard<T>, f: F) -> BorrowGuard<U>
    where F: FnOnce(&mut T) -> &mut U,
    {
        let u = f(&mut *r) as *mut U;

        BorrowGuard {
            value_ptr: u,
            handle: r.handle,
        }
    }

    /// Make a new `BorrowGuard` for a component of the borrowed data.
    ///
    /// The `BorrowGuard` is already mutably borrowed, so this cannot fail.
    pub fn try_map<F, U, E>(mut r: BorrowGuard<T>, f: F)
        -> Result<BorrowGuard<U>, (BorrowGuard<T>, E)>
    where F: FnOnce(&mut T) -> Result<&mut U, E>
    {

        let res = f(&mut *r)
            .map(|u| u as *mut U);

        match res {
            Ok(u) => {
                Ok(BorrowGuard {
                    value_ptr: u,
                    handle: r.handle,
                })
            }
            Err(e) => {
                Err((r, e))
            }
        }
    }
}

impl<T: Default + 'static> Default for Borrow<T> {
    fn default() -> Borrow<T> {
        Borrow::new(T::default())
    }
}

impl<T: fmt::Debug + 'static> fmt::Debug for Borrow<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.try_borrow() {
            Ok(guard) => {
                fmt.debug_struct("Borrow")
                    .field("data", &*guard)
                    .finish()
            }
            Err(e) => {
                if e.is_poisoned() {
                    fmt.debug_struct("Borrow")
                        .field("data", &"Poisoned")
                        .finish()
                } else {
                    fmt.debug_struct("Borrow")
                        .field("data", &"<<borrowed>>")
                        .finish()
                }
            },
        }
    }
}

unsafe impl<T: Send> Send for Borrow<T> { }
unsafe impl<T: Send> Sync for Borrow<T> { }

// ===== impl BorrowGuard =====

impl<T> ops::Deref for BorrowGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.value_ptr }
    }
}

impl<T> ops::DerefMut for BorrowGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value_ptr }
    }
}

impl<T: fmt::Debug> fmt::Debug for BorrowGuard<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("BorrowGuard")
            .field("data", &**self)
            .finish()
    }
}

unsafe impl<T: Send> Send for BorrowGuard<T> { }
unsafe impl<T: Sync> Sync for BorrowGuard<T> { }

// ===== impl BorrowHandle =====

impl Drop for BorrowHandle {
    fn drop(&mut self) {
        let state = unsafe { &*self.state_ptr };

        if thread::panicking() {
            state.borrowed.store(POISONED, Release);
        } else {
            state.borrowed.store(UNUSED, Release);
        }

        state.task.notify();
    }
}

// ===== impl BorrowError =====

impl BorrowError {
    fn new() -> BorrowError {
        BorrowError {
            _priv: (),
        }
    }
}

// ===== impl TryBorrowError =====

impl TryBorrowError {
    fn new(is_poisoned: bool) -> TryBorrowError {
        TryBorrowError {
            is_poisoned,
        }
    }

    pub fn is_poisoned(&self) -> bool {
        self.is_poisoned
    }
}
