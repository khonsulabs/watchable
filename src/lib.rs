#![doc = include_str!(".crate-docs.md")]
#![forbid(unsafe_code)]
#![warn(
    clippy::cargo,
    missing_docs,
    // clippy::missing_docs_in_private_items,
    clippy::pedantic,
    future_incompatible,
    rust_2018_idioms,
)]
#![allow(clippy::option_if_let_else, clippy::module_name_repetitions)]

use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
    time::{Duration, Instant},
};

use event_listener::{Event, EventListener};
use futures_util::{FutureExt, Stream};
use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

/// A watchable wrapper for a value.
#[derive(Default, Debug)]
pub struct Watchable<T> {
    data: Arc<Data<T>>,
}

impl<T> Clone for Watchable<T> {
    fn clone(&self) -> Self {
        self.data.watchables.fetch_add(1, Ordering::AcqRel);
        Self {
            data: self.data.clone(),
        }
    }
}

impl<T> Drop for Watchable<T> {
    fn drop(&mut self) {
        if self.data.watchables.fetch_sub(1, Ordering::AcqRel) == 1 {
            // Last watchable
            self.shutdown();
        }
    }
}

impl<T> Watchable<T> {
    /// Returns a new instance with the initial value provided.
    pub fn new(initial_value: T) -> Self {
        Self {
            data: Arc::new(Data {
                value: RwLock::new(initial_value),
                changed: RwLock::new(Some(Event::new())),
                version: AtomicUsize::new(0),
                watchers: AtomicUsize::new(0),
                watchables: AtomicUsize::new(1),
            }),
        }
    }

    /// Returns a new watcher that can monitor for changes to the contained
    /// value.
    pub fn watch(&self) -> Watcher<T> {
        self.data.watchers.fetch_add(1, Ordering::AcqRel);
        Watcher {
            version: self.data.current_version(),
            watched: self.data.clone(),
        }
    }

    /// Replaces the current value contained and notifies all watching
    /// [`Watcher`]s. Returns the previously stored value.
    pub fn replace(&self, new_value: T) -> T {
        let mut stored = self.data.value.write();
        let mut old_value = new_value;
        std::mem::swap(&mut *stored, &mut old_value);
        self.data.increment_version();
        old_value
    }

    /// Updates the current value, if it is different from the contained value.
    /// Returns `Ok(previous_value)` if `new_value != previous_value`, otherwise
    /// returns `Err(new_value)`.
    ///
    /// # Errors
    ///
    /// Returns `Err(new_value)` if the currently stored value is equal to `new_value`.
    pub fn update(&self, new_value: T) -> Result<T, T>
    where
        T: PartialEq,
    {
        let stored = self.data.value.upgradable_read();
        if *stored == new_value {
            Err(new_value)
        } else {
            let mut stored = RwLockUpgradableReadGuard::upgrade(stored);
            let mut old_value = new_value;
            std::mem::swap(&mut *stored, &mut old_value);
            self.data.increment_version();
            Ok(old_value)
        }
    }

    /// Returns a write guard that allows updating the value. If the inner value
    /// is accessed through [`DerefMut::deref_mut()`], all [`Watcher`]s will be
    /// notified when the returned guard is dropped.
    ///
    /// [`WatchableWriteGuard`] holds an exclusive lock. No other threads will
    /// be able to read or write the contained value until the guard is dropped.
    pub fn write(&self) -> WatchableWriteGuard<'_, T> {
        WatchableWriteGuard {
            watchable: self,
            guard: self.data.value.write(),
            accessed_mut: false,
        }
    }

    /// Returns a guard which can be used to access the value held within the
    /// variable. This guard does not block other threads from reading the
    /// value.
    pub fn read(&self) -> WatchableReadGuard<'_, T> {
        WatchableReadGuard(self.data.value.read())
    }

    /// Returns the currently contained value.
    #[must_use]
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.data.value.read().clone()
    }

    /// Returns the number of [`Watcher`]s for this value.
    #[must_use]
    pub fn watchers(&self) -> usize {
        self.data.watchers.load(Ordering::Acquire)
    }

    /// Returns true if there are any [`Watcher`]s for this value.
    #[must_use]
    pub fn has_watchers(&self) -> bool {
        self.watchers() > 0
    }

    /// Disconnects all [`Watcher`]s.
    ///
    /// All future value updates will not be observed by the watchers, but the
    /// last value will still be readable before the watcher signals that it is
    /// disconnected.
    pub fn shutdown(&self) {
        let mut changed = self.data.changed.write();
        if let Some(changed) = changed.take() {
            changed.notify(usize::MAX);
        }
    }
}

impl<T> Data<T> {
    fn current_version(&self) -> usize {
        self.version.load(Ordering::Acquire)
    }

    fn increment_version(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
        let changed = self.changed.read();
        if let Some(changed) = changed.as_ref() {
            changed.notify(usize::MAX);
        }
    }
}

/// A read guard that allows reading the currently stored value in a
/// [`Watchable`]. No values can be stored within the source [`Watchable`] while
/// this guard exists.
///
/// The inner value is accessible through [`Deref`].
#[must_use]
pub struct WatchableReadGuard<'a, T>(RwLockReadGuard<'a, T>);

impl<'a, T> Deref for WatchableReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A write guard that allows updating the currently stored value in a
/// [`Watchable`].
///
/// The inner value is readable through [`Deref`], and modifiable through
/// [`DerefMut`]. Any usage of [`DerefMut`] will cause all [`Watcher`]s to be
/// notified of an updated value when the guard is dropped.
///
/// [`WatchableWriteGuard`] is an exclusive guard. No other threads will be
/// able to read or write the contained value until the guard is dropped.
#[must_use]
pub struct WatchableWriteGuard<'a, T> {
    watchable: &'a Watchable<T>,
    accessed_mut: bool,
    guard: RwLockWriteGuard<'a, T>,
}

impl<'a, T> Deref for WatchableWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T> DerefMut for WatchableWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.accessed_mut = true;
        &mut self.guard
    }
}

impl<'a, T> Drop for WatchableWriteGuard<'a, T> {
    fn drop(&mut self) {
        if self.accessed_mut {
            self.watchable.data.increment_version();
        }
    }
}

#[derive(Debug)]
struct Data<T> {
    changed: RwLock<Option<Event>>,
    version: AtomicUsize,
    watchers: AtomicUsize,
    watchables: AtomicUsize,
    value: RwLock<T>,
}

impl<T> Default for Data<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            changed: RwLock::new(Some(Event::new())),
            version: AtomicUsize::new(0),
            watchers: AtomicUsize::new(0),
            watchables: AtomicUsize::new(1),
            value: RwLock::default(),
        }
    }
}

/// An observer of a [`Watchable`] value.
///
/// ## Cloning behavior
///
/// Cloning a watcher also clones the current watching state. If the watcher
/// hasn't read the value currently stored, the cloned instance will also
/// consider the current value unread.
#[derive(Debug, Clone)]
#[must_use]
pub struct Watcher<T> {
    version: usize,
    watched: Arc<Data<T>>,
}

impl<T> Drop for Watcher<T> {
    fn drop(&mut self) {
        self.watched.watchers.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
enum CreateListenerError {
    NewValueAvailable,
    Disconnected,
}

/// A watch operation failed because all [`Watchable`] instances have been
/// dropped.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("all watchable instances have been dropped")]
pub struct Disconnected;

/// A watch operation with a timeout failed.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum TimeoutError {
    /// A watch operation failed because all [`Watchable`] instances have been
    /// dropped.
    #[error("all watchable instances have been dropped")]
    Disconnected,
    /// No new values were written before the timeout elapsed
    #[error("no new values were written before the timeout elapsed")]
    Timeout,
}

impl<T> Watcher<T> {
    fn create_listener_if_needed(&self) -> Result<EventListener, CreateListenerError> {
        let changed = self.watched.changed.read();
        match (changed.as_ref(), self.is_current()) {
            (_, false) => Err(CreateListenerError::NewValueAvailable),
            (None, _) => Err(CreateListenerError::Disconnected),
            (Some(changed), true) => {
                let listener = changed.listen();

                // Between now and creating the listener, an update may have
                // come in, so we need to check again before returning the
                // listener.
                if self.is_current() {
                    Ok(listener)
                } else {
                    Err(CreateListenerError::NewValueAvailable)
                }
            }
        }
    }

    /// Returns true if the latest value has been read from this instance.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.version == self.watched.current_version()
    }

    /// Updates this instance's state to reflect that it has read the currently
    /// stored value. The next call to a watch call will block until the next
    /// value is stored.
    ///
    /// Returns true if the internal state was updated, and false if no changes
    /// were necessary.
    pub fn mark_read(&mut self) -> bool {
        let current_version = self.watched.current_version();
        if self.version == current_version {
            false
        } else {
            self.version = current_version;
            true
        }
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// read with [`Self::mark_read()`], this call will block the calling
    /// thread until a new value has been published.
    ///
    /// # Errors
    ///
    /// Returns [`Disconnected`] if all instances of [`Watchable`] have been
    /// dropped and the current value has been read.
    pub fn watch(&self) -> Result<(), Disconnected> {
        loop {
            match self.create_listener_if_needed() {
                Ok(listener) => {
                    listener.wait();
                    if !self.is_current() {
                        break;
                    }
                }
                Err(CreateListenerError::Disconnected) => return Err(Disconnected),
                Err(CreateListenerError::NewValueAvailable) => break,
            }
        }

        Ok(())
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// read with [`Self::mark_read()`], this call will block the calling
    /// thread until a new value has been published or until `duration` has
    /// elapsed.
    ///
    /// # Errors
    ///
    /// - [`TimeoutError::Disconnected`]: All instances of [`Watchable`] have
    /// been dropped and the current value has been read.
    /// - [`TimeoutError::Timeout`]: A timeout occurred before a new value was
    /// written.
    pub fn watch_timeout(&self, duration: Duration) -> Result<(), TimeoutError> {
        self.watch_until(Instant::now() + duration)
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// read with [`Self::mark_read()`], this call will block the calling
    /// thread until a new value has been published or until `deadline`.
    ///
    /// # Errors
    ///
    /// - [`TimeoutError::Disconnected`]: All instances of [`Watchable`] have
    /// been dropped and the current value has been read.
    /// - [`TimeoutError::Timeout`]: A timeout occurred before a new value was
    /// written.
    pub fn watch_until(&self, deadline: Instant) -> Result<(), TimeoutError> {
        loop {
            match self.create_listener_if_needed() {
                Ok(listener) => {
                    if listener.wait_deadline(deadline) {
                        if !self.is_current() {
                            break;
                        } else if Instant::now() < deadline {
                            // Spurious wake-up
                        }
                    } else {
                        return Err(TimeoutError::Timeout);
                    }
                }
                Err(CreateListenerError::Disconnected) => return Err(TimeoutError::Disconnected),
                Err(CreateListenerError::NewValueAvailable) => break,
            }
        }

        Ok(())
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// read with [`Self::mark_read()`], the async task will block until
    /// a new value has been published.
    ///
    /// # Errors
    ///
    /// Returns [`Disconnected`] if all instances of [`Watchable`] have been
    /// dropped and the current value has been read.
    pub async fn watch_async(&self) -> Result<(), Disconnected> {
        loop {
            match self.create_listener_if_needed() {
                Ok(listener) => {
                    listener.await;
                    if !self.is_current() {
                        break;
                    }
                }
                Err(CreateListenerError::Disconnected) => return Err(Disconnected),
                Err(CreateListenerError::NewValueAvailable) => break,
            }
        }
        Ok(())
    }

    /// Returns a read guard that allows reading the currently stored value.
    /// This function does not consider the value read, and the next call to a
    /// watch function will be unaffected.
    pub fn peek(&self) -> WatchableReadGuard<'_, T> {
        let guard = self.watched.value.read();
        WatchableReadGuard(guard)
    }

    /// Returns a read guard that allows reading the currently stored value.
    /// This function marks the stored value as read, ensuring that the next
    /// call to a watch function will block until the a new value has been
    /// published.
    pub fn read(&mut self) -> WatchableReadGuard<'_, T> {
        let guard = self.watched.value.read();
        self.version = self.watched.current_version();
        WatchableReadGuard(guard)
    }

    /// Returns the currently contained value. This function marks the stored
    /// value as read, ensuring that the next call to a watch function will
    /// block until the a new value has been published.
    #[must_use]
    pub fn get(&mut self) -> T
    where
        T: Clone,
    {
        self.read().clone()
    }

    /// Watches for a new value to be stored in the source [`Watchable`] and
    /// returns a clone of it. If the current value hasn't been accessed through
    /// [`Self::read()`] or marked read with [`Self::mark_read()`], this call
    /// will block the calling thread until a new value has been published.
    ///
    /// # Errors
    ///
    /// Returns [`Disconnected`] if all instances of [`Watchable`] have been
    /// dropped and the current value has been read.
    pub fn next_value(&mut self) -> Result<T, Disconnected>
    where
        T: Clone,
    {
        self.watch().map(|_| self.read().clone())
    }

    /// Watches for a new value to be stored in the source [`Watchable`] and
    /// returns a clone of it. If the current value hasn't been accessed through
    /// [`Self::read()`] or marked read with [`Self::mark_read()`], this call
    /// will asynchronously wait for a new value to be published.
    ///
    /// The async task is safe to be cancelled without losing track of the last
    /// read value.
    ///
    /// # Errors
    ///
    /// Returns [`Disconnected`] if all instances of [`Watchable`] have been
    /// dropped and the current value has been read.
    pub async fn next_value_async(&mut self) -> Result<T, Disconnected>
    where
        T: Clone,
    {
        self.watch_async().await.map(|_| self.read().clone())
    }

    /// Returns this watcher in a type that implements [`Stream`].
    pub fn into_stream(self) -> WatcherStream<T> {
        WatcherStream {
            watcher: self,
            listener: None,
        }
    }
}

impl<T> Iterator for Watcher<T>
where
    T: Clone,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_value().ok()
    }
}

/// Asynchronous iterator for a [`Watcher`]. Implements [`Stream`].
#[derive(Debug)]
#[must_use]
pub struct WatcherStream<T> {
    watcher: Watcher<T>,
    listener: Option<EventListener>,
}

impl<T> WatcherStream<T> {
    /// Returns the wrapped [`Watcher`].
    pub fn into_inner(self) -> Watcher<T> {
        self.watcher
    }
}

impl<T> Stream for WatcherStream<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // If we have a listener or we have already read the current value, we
        // need to poll the listener as a future first.
        loop {
            match self
                .listener
                .take()
                .ok_or(CreateListenerError::Disconnected)
                .or_else(|_| self.watcher.create_listener_if_needed())
            {
                Ok(mut listener) => {
                    match listener.poll_unpin(cx) {
                        Poll::Ready(_) => {
                            if !self.watcher.is_current() {
                                break;
                            }

                            // A new value is available. Fall through.
                        }
                        Poll::Pending => {
                            // The listener wasn't ready, store it again.
                            self.listener = Some(listener);
                            return Poll::Pending;
                        }
                    }
                }
                Err(CreateListenerError::NewValueAvailable) => break,
                Err(CreateListenerError::Disconnected) => return Poll::Ready(None),
            }
        }

        Poll::Ready(Some(self.watcher.read().clone()))
    }
}

#[test]
fn basics() {
    let watchable = Watchable::new(1_u32);
    assert!(!watchable.has_watchers());
    let mut watcher1 = watchable.watch();
    let mut watcher2 = watchable.watch();
    assert!(!watcher1.mark_read());

    assert_eq!(watchable.watchers(), 2);
    assert_eq!(watchable.replace(2), 1);
    // A call to watch should not block since the value has already been sent
    watcher1.watch().unwrap();
    // Peek shouldn't cause the watcher to block.
    assert_eq!(*watcher1.peek(), 2);
    watcher1.watch().unwrap();
    // Reading should switch the state back to needing to block, which we'll
    // test in an other unit test
    assert_eq!(*watcher1.read(), 2);
    assert!(!watcher1.mark_read());
    drop(watcher1);
    assert_eq!(watchable.watchers(), 1);

    // Now, despite watcher1 having updated, watcher2 should have independent state
    assert!(watcher2.mark_read());
    assert_eq!(*watcher2.read(), 2);
    drop(watcher2);
    assert_eq!(watchable.watchers(), 0);
}

#[test]
fn accessing_values() {
    let watchable = Watchable::new(String::from("hello"));
    assert_eq!(watchable.get(), "hello");
    assert_eq!(&*watchable.read(), "hello");
    assert_eq!(&*watchable.write(), "hello");

    let mut watcher = watchable.watch();
    assert_eq!(watcher.get(), "hello");
    assert_eq!(&*watcher.read(), "hello");
}

#[test]
fn clones() {
    let watchable = Watchable::default();
    let cloned_watchable = watchable.clone();
    let mut watcher1 = watchable.watch();
    let mut watcher2 = watcher1.clone();

    watchable.replace(1);
    assert_eq!(watcher1.next_value().unwrap(), 1);
    assert_eq!(watcher2.next_value().unwrap(), 1);
    cloned_watchable.replace(2);
    assert_eq!(watcher1.next_value().unwrap(), 2);
    assert_eq!(watcher2.next_value().unwrap(), 2);
}

#[test]
fn drop_watchable() {
    let watchable = Watchable::default();
    assert!(!watchable.has_watchers());
    let mut watcher = watchable.watch();

    watchable.replace(1_u32);
    assert_eq!(watcher.next_value().unwrap(), 1);

    drop(watchable);
    assert!(matches!(watcher.next_value().unwrap_err(), Disconnected));
}

#[test]
fn drop_watchable_timeouts() {
    let watchable = Watchable::new(0_u8);
    assert!(!watchable.has_watchers());
    let watcher = watchable.watch();
    let start = Instant::now();
    let wait_timeout_thread = std::thread::spawn(move || {
        assert!(matches!(
            watcher.watch_timeout(Duration::from_secs(15)).unwrap_err(),
            TimeoutError::Disconnected
        ));
    });
    let watcher = watchable.watch();
    let wait_until_thread = std::thread::spawn(move || {
        assert!(matches!(
            watcher
                .watch_until(Instant::now().checked_add(Duration::from_secs(15)).unwrap())
                .unwrap_err(),
            TimeoutError::Disconnected
        ));
    });

    // Give time for the threads to spawn.
    std::thread::sleep(Duration::from_millis(100));
    drop(watchable);

    wait_timeout_thread.join().unwrap();
    wait_until_thread.join().unwrap();

    let elapsed = Instant::now().checked_duration_since(start).unwrap();
    assert!(elapsed.as_secs() < 1);
}

#[test]
fn timeouts() {
    let watchable = Watchable::new(1_u32);
    let watcher = watchable.watch();
    let start = Instant::now();
    assert!(matches!(
        watcher.watch_timeout(Duration::from_millis(100)),
        Err(TimeoutError::Timeout)
    ));
    assert!(matches!(
        watcher.watch_until(Instant::now() + Duration::from_millis(100)),
        Err(TimeoutError::Timeout)
    ));
    let elapsed = Instant::now().checked_duration_since(start).unwrap();
    // We don't control the delay logic, so to ensure this test is stable, we're
    // comparing against a duration slightly less than 200 ms even though in
    // theory that shouldn't be possible.
    assert!(elapsed.as_millis() >= 180);

    // Test that watch_timeout/until return true when a new event is available
    watchable.replace(2);
    watcher.watch_timeout(Duration::from_secs(1)).unwrap();
    watchable.replace(3);
    watcher
        .watch_until(Instant::now() + Duration::from_secs(1))
        .unwrap();
}

#[test]
fn deref_publish() {
    let watchable = Watchable::new(1_u32);
    let mut watcher = watchable.watch();
    // Reading the value (Deref) shouldn't publish a new value
    {
        let write_guard = watchable.write();
        assert_eq!(*write_guard, 1);
    }
    assert!(!watcher.mark_read());
    // Writing a value (DerefMut) should publish a new value
    {
        let mut write_guard = watchable.write();
        *write_guard = 2;
    }
    assert!(watcher.mark_read());
}

#[test]
fn blocking_tests() {
    let watchable = Watchable::new(1_u32);
    let mut watcher = watchable.watch();
    let worker_thread = std::thread::spawn(move || {
        watcher.watch().unwrap();
        assert_eq!(*watcher.read(), 2);
        watcher.watch().unwrap();
        *watcher.read()
    });

    watchable.replace(2);
    std::thread::sleep(Duration::from_millis(10));
    assert!(watchable.update(42).is_ok());
    assert!(watchable.update(42).is_err());

    assert_eq!(worker_thread.join().unwrap(), 42);
}

#[test]
fn iterator_test() {
    let watchable = Watchable::new(1_u32);
    let mut watcher = watchable.watch();
    let worker_thread = std::thread::spawn(move || {
        let mut last_value = watcher.next_value().unwrap();
        for value in watcher {
            assert_ne!(last_value, value);
            println!("Received {value}");
            last_value = value;
        }
        assert_eq!(last_value, 1000);
    });

    for i in 1..=1000 {
        watchable.replace(i);
    }
    drop(watchable);
    worker_thread.join().unwrap();
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn stream_test() {
    use futures_util::StreamExt;

    let watchable = Watchable::default();
    let mut watcher = watchable.watch();
    let worker_thread = tokio::task::spawn(async move {
        let mut last_value = watcher.next_value_async().await.unwrap();
        let mut stream = watcher.into_stream();
        while let Some(value) = stream.next().await {
            assert_ne!(last_value, value);
            println!("Received {value}");
            last_value = value;
        }
        assert_eq!(last_value, 1000);

        // Ensure it's safe to call next again with no blocking and no panics.
        assert!(stream.next().await.is_none());

        // Convert back to a normal watcher and check that the state still
        // matches.
        let mut watcher = stream.into_inner();
        assert!(!watcher.mark_read());
    });

    for i in 1..=1000 {
        watchable.replace(i);
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // Allow the stream to end
    drop(watchable);

    // Wait for the task to finish.
    worker_thread.await.unwrap();
}

#[test]
fn stress_test() {
    let watchable = Watchable::new(1_u32);
    let mut workers = Vec::new();
    for _ in 1..=10 {
        let mut watcher = watchable.watch();
        workers.push(std::thread::spawn(move || {
            let mut last_value = *watcher.read();
            while watcher.watch().is_ok() {
                let current_value = *watcher.read();
                assert_ne!(last_value, current_value);
                last_value = current_value;
            }
            assert_eq!(last_value, 10000);
        }));
    }

    for i in 1..=10000 {
        let _ = watchable.update(i);
    }
    drop(watchable);

    for worker in workers {
        worker.join().unwrap();
    }
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn stress_test_async() {
    let watchable = Watchable::new(1_u32);
    let mut workers = Vec::new();
    for _ in 1..=64 {
        let mut watcher = watchable.watch();
        workers.push(tokio::task::spawn(async move {
            let mut last_value = *watcher.read();
            loop {
                watcher.watch_async().await.unwrap();
                let current_value = *watcher.read();
                assert_ne!(last_value, current_value);
                if current_value == 10000 {
                    break;
                }
                last_value = current_value;
            }
        }));
    }

    tokio::task::spawn_blocking(move || {
        for i in 1..=10000 {
            let _ = watchable.update(i);
        }
    })
    .await
    .unwrap();

    for worker in workers {
        worker.await.unwrap();
    }
}

#[test]
fn shutdown() {
    let watchable = Watchable::new(0);
    let mut watcher = watchable.watch();

    // Set a new value, then shutdown
    watchable.replace(1);
    watchable.shutdown();

    // The value should still be accessible
    assert_eq!(watcher.next_value().expect("initial value missing"), 1);
    watcher
        .next_value()
        .expect_err("watcher should be disconnected");
}
