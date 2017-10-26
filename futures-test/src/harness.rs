use futures::{Future, Stream, Poll, Async};
use futures::executor::{spawn, Spawn, Notify};

use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Wraps a future, providing an API to interact with it while off task.
///
/// This wrapper is intended to use from the context of tests. The future is
/// effectively wrapped by a task and this harness tracks received notifications
/// as well as provides APIs to perform non-blocking polling as well blocking
/// polling with or without timeout.
#[derive(Debug)]
pub struct Harness<T> {
    spawn: Spawn<T>,
    notify: Arc<ThreadNotify>,
}

/// Error produced by `TestHarness` operations with timeout.
#[derive(Debug)]
pub struct TimeoutError<T> {
    /// If `None`, represents a timeout error
    inner: Option<T>,
}

#[derive(Debug)]
struct ThreadNotify {
    state: AtomicUsize,
    mutex: Mutex<()>,
    condvar: Condvar,
}

const IDLE: usize = 0;
const NOTIFY: usize = 1;
const SLEEP: usize = 2;

impl<T> Harness<T> {
    /// Wraps `obj` in a test harness, enabling interacting with the future
    /// while not on a `Task`.
    pub fn new(obj: T) -> Self {
        Harness {
            spawn: spawn(obj),
            notify: Arc::new(ThreadNotify::new()),
        }
    }

    pub fn with<F, R>(&mut self, f: F) -> R
    where F: FnOnce(&mut Self) -> R,
    {
        f(self)
    }

    /// Returns `true` if the inner future has received a readiness notification
    /// since the last action has been performed.
    pub fn is_notified(&self) -> bool {
        self.notify.is_notified()
    }

    /// Returns a reference to the inner future.
    pub fn get_ref(&self) -> &T {
        self.spawn.get_ref()
    }

    /// Returns a mutable reference to the inner future.
    pub fn get_mut(&mut self) -> &mut T {
        self.spawn.get_mut()
    }

    /// Consumes `self`, returning the inner future.
    pub fn into_inner(self) -> T {
        self.spawn.into_inner()
    }
}

impl<F, T, E> Harness<::futures::future::PollFn<F>>
where F: FnMut() -> Poll<T, E>,
{
    /// Wraps the `poll_fn` in a harness.
    pub fn poll_fn(f: F) -> Self {
        Harness::new(::futures::future::poll_fn(f))
    }
}

impl<T: Future> Harness<T> {
    /// Polls the inner future.
    ///
    /// This function returns immediately. If the inner future is not currently
    /// ready, `NotReady` is returned. Readiness notifications are tracked and
    /// can be queried using `is_notified`.
    pub fn poll(&mut self) -> Poll<T::Item, T::Error> {
        self.spawn.poll_future_notify(&self.notify, 0)
    }

    /// Waits for the internal future to complete, blocking this thread's
    /// execution until it does.
    pub fn wait(&mut self) -> Result<T::Item, T::Error> {
        self.notify.clear();

        loop {
            match self.spawn.poll_future_notify(&self.notify, 0)? {
                Async::NotReady => self.notify.park(),
                Async::Ready(e) => return Ok(e),
            }
        }
    }

    /// Waits for the internal future to complete, blocking this thread's
    /// execution for at most `dur`.
    pub fn wait_timeout(&mut self, dur: Duration)
        -> Result<T::Item, TimeoutError<T::Error>>
    {
        let until = Instant::now() + dur;

        self.notify.clear();

        loop {
            let res = self.spawn.poll_future_notify(&self.notify, 0)
                .map_err(TimeoutError::new);

            match res? {
                Async::NotReady => {
                    let now = Instant::now();

                    if now >= until {
                        return Err(TimeoutError::timeout());
                    }

                    self.notify.park_timeout(Some(until - now));
                }
                Async::Ready(e) => return Ok(e),
            }
        }
    }
}

impl<T: Stream> Harness<T> {
    /// Polls the inner future.
    ///
    /// This function returns immediately. If the inner future is not currently
    /// ready, `NotReady` is returned. Readiness notifications are tracked and
    /// can be queried using `is_notified`.
    pub fn poll_next(&mut self) -> Poll<Option<T::Item>, T::Error> {
        self.spawn.poll_stream_notify(&self.notify, 0)
    }
}

impl<T> TimeoutError<T> {
    fn new(inner: T) -> Self {
        TimeoutError { inner: Some(inner) }
    }

    fn timeout() -> Self {
        TimeoutError { inner: None }
    }

    pub fn is_timeout(&self) -> bool {
        self.inner.is_none()
    }

    /// Consumes `self`, returning the inner error. Returns `None` if `self`
    /// represents a timeout.
    pub fn into_inner(self) -> Option<T> {
        self.inner
    }
}

impl ThreadNotify {
    fn new() -> Self {
        ThreadNotify {
            state: AtomicUsize::new(IDLE),
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }

    /// Clears any previously received notify, avoiding potential spurrious
    /// notifications. This should only be called immediately before running the
    /// task.
    fn clear(&self) {
        self.state.store(IDLE, Ordering::SeqCst);
    }

    fn is_notified(&self) -> bool {
        match self.state.load(Ordering::SeqCst) {
            IDLE => false,
            NOTIFY => true,
            _ => unreachable!(),
        }
    }

    fn park(&self) {
        self.park_timeout(None);
    }

    fn park_timeout(&self, dur: Option<Duration>) {
        // If currently notified, then we skip sleeping. This is checked outside
        // of the lock to avoid acquiring a mutex if not necessary.
        match self.state.compare_and_swap(NOTIFY, IDLE, Ordering::SeqCst) {
            NOTIFY => return,
            IDLE => {},
            _ => unreachable!(),
        }

        // The state is currently idle, so obtain the lock and then try to
        // transition to a sleeping state.
        let mut m = self.mutex.lock().unwrap();

        // Transition to sleeping
        match self.state.compare_and_swap(IDLE, SLEEP, Ordering::SeqCst) {
            NOTIFY => {
                // Notified before we could sleep, consume the notification and
                // exit
                self.state.store(IDLE, Ordering::SeqCst);
                return;
            }
            IDLE => {},
            _ => unreachable!(),
        }

        // Track (until, remaining)
        let mut time = dur.map(|dur| (Instant::now() + dur, dur));

        loop {
            m = match time {
                Some((until, rem)) => {
                    let (guard, _) = self.condvar.wait_timeout(m, rem).unwrap();
                    let now = Instant::now();

                    if now >= until {
                        // Timed out... exit sleep state
                        self.state.store(IDLE, Ordering::SeqCst);
                        return;
                    }

                    time = Some((until, until - now));
                    guard
                }
                None => self.condvar.wait(m).unwrap(),
            };

            // Transition back to idle, loop otherwise
            if NOTIFY == self.state.compare_and_swap(NOTIFY, IDLE, Ordering::SeqCst) {
                return;
            }
        }
    }
}

impl Notify for ThreadNotify {
    fn notify(&self, _unpark_id: usize) {
        // First, try transitioning from IDLE -> NOTIFY, this does not require a
        // lock.
        match self.state.compare_and_swap(IDLE, NOTIFY, Ordering::SeqCst) {
            IDLE | NOTIFY => return,
            SLEEP => {}
            _ => unreachable!(),
        }

        // The other half is sleeping, this requires a lock
        let _m = self.mutex.lock().unwrap();

        // Transition from SLEEP -> NOTIFY
        match self.state.compare_and_swap(SLEEP, NOTIFY, Ordering::SeqCst) {
            SLEEP => {}
            _ => return,
        }

        // Wakeup the sleeper
        self.condvar.notify_one();
    }
}
