#![doc = include_str!(".crate-docs.md")]
#![forbid(unsafe_code)]
#![warn(
    // clippy::cargo,
    missing_docs,
    // clippy::missing_docs_in_private_items,
    clippy::pedantic,
    future_incompatible,
    rust_2018_idioms,
)]
#![allow(
    clippy::missing_errors_doc, // TODO clippy::missing_errors_doc
    clippy::option_if_let_else,
    clippy::module_name_repetitions,
)]

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
#[derive(Clone, Debug)]
pub struct Watchable<T> {
    data: Arc<Data<T>>,
}

impl<T> Watchable<T> {
    /// Returns a new instance with the initial value provided.
    pub fn new(initial_value: T) -> Self {
        Self {
            data: Arc::new(Data {
                changed: Event::new(),
                version: AtomicUsize::new(0),
                watchers: AtomicUsize::new(0),
                value: RwLock::new(initial_value),
            }),
        }
    }

    /// Returns a new watcher that can monitor for changes to the contained
    /// value.
    pub fn subscribe(&self) -> Watcher<T> {
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
    pub fn write(&self) -> WatchableWriteGuard<'_, T> {
        WatchableWriteGuard {
            watchable: self,
            guard: self.data.value.write(),
            accessed_mut: false,
        }
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
}

impl<T> Data<T> {
    fn current_version(&self) -> usize {
        self.version.load(Ordering::Acquire)
    }

    fn increment_version(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
        self.changed.notify(usize::MAX);
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
        &*self.0
    }
}

/// A write guard that allows updating the currently stored value in a
/// [`Watchable`].
///
/// The inner value is readable through [`Deref`], and modifiable through
/// [`DerefMut`]. Any usage of [`DerefMut`] will cause all [`Watcher`]s to be
/// notified of an updated value when the guard is dropped.
#[must_use]
pub struct WatchableWriteGuard<'a, T> {
    watchable: &'a Watchable<T>,
    accessed_mut: bool,
    guard: RwLockWriteGuard<'a, T>,
}

impl<'a, T> Deref for WatchableWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl<'a, T> DerefMut for WatchableWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.accessed_mut = true;
        &mut *self.guard
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
    changed: Event,
    version: AtomicUsize,
    watchers: AtomicUsize,
    value: RwLock<T>,
}

/// An observer of a [`Watchable`] value.
///
/// ## Cloning behavior
///
/// Cloning a watcher also clones the current watching state. If the watcher
/// hasn't seen the value currently stored, the cloned instance will also
/// consider the current value unseen.
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

impl<T> Watcher<T> {
    fn create_listener_if_needed(&self) -> Option<EventListener> {
        // Verify that we have the currently published version. To ensure
        // there's no race condition between querying the current version and
        // another thread publishing a value, we're going to acquire a read
        // guard before we check the version.
        let _guard = self.watched.value.read();
        (self.watched.current_version() == self.version).then(|| self.watched.changed.listen())
    }

    /// Updates this instance's state to reflect that it has seen the currently
    /// stored value. The next call to a watch call will block until the next
    /// value is stored.
    pub fn update_if_needed(&mut self) -> bool {
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
    /// seen with [`Self::update_if_needed()`], this call will block the calling
    /// thread until a new value has been published.
    pub fn watch(&self) {
        if let Some(listener) = self.create_listener_if_needed() {
            listener.wait();
        }
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// seen with [`Self::update_if_needed()`], this call will block the calling
    /// thread until a new value has been published or until `duration` has
    /// elapsed.
    pub fn watch_timeout(&self, duration: Duration) {
        if let Some(listener) = self.create_listener_if_needed() {
            listener.wait_timeout(duration);
        }
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// seen with [`Self::update_if_needed()`], this call will block the calling
    /// thread until a new value has been published or until `deadline`.
    pub fn watch_until(&self, deadline: Instant) {
        if let Some(listener) = self.create_listener_if_needed() {
            listener.wait_deadline(deadline);
        }
    }

    /// Watches for a new value to be stored in the source [`Watchable`]. If the
    /// current value hasn't been accessed through [`Self::read()`] or marked
    /// seen with [`Self::update_if_needed()`], the async task will block until
    /// a new value has been published.
    pub async fn watch_async(&self) {
        if let Some(listener) = self.create_listener_if_needed() {
            listener.await;
        }
    }

    /// Returns a read guard that allows reading the currently stored value.
    /// This function does not consider the value seen, and the next call to a
    /// watch function will be unaffected.
    pub fn peek(&self) -> WatchableReadGuard<'_, T> {
        let guard = self.watched.value.read();
        WatchableReadGuard(guard)
    }

    /// Returns a read guard that allows reading the currently stored value.
    /// This function marks the stored value as seen, ensuring that the next
    /// call to a watch function will block until the a new value has been
    /// published.
    pub fn read(&mut self) -> WatchableReadGuard<'_, T> {
        let guard = self.watched.value.read();
        self.version = self.watched.current_version();
        WatchableReadGuard(guard)
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
        self.watch();
        Some(self.read().clone())
    }
}

/// Asynchronous iterator for a [`Watcher`]. Implements [`Stream`].
#[derive(Debug)]
#[must_use]
pub struct WatcherStream<T> {
    watcher: Watcher<T>,
    listener: Option<EventListener>,
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
        // If we have a listener or we have already seen the current value, we
        // need to poll the listener as a future first.
        if let Some(mut listener) = self
            .listener
            .take()
            .or_else(|| self.watcher.create_listener_if_needed())
        {
            match listener.poll_unpin(cx) {
                Poll::Ready(_) => {
                    // A new value is available
                }
                Poll::Pending => {
                    // The listener wasn't ready, store it again.
                    self.listener = Some(listener);
                    return Poll::Pending;
                }
            }
        }

        Poll::Ready(Some(self.watcher.read().clone()))
    }
}

#[test]
fn basics() {
    let watchable = Watchable::new(1_u32);
    assert!(!watchable.has_watchers());
    let mut watcher1 = watchable.subscribe();
    let mut watcher2 = watchable.subscribe();
    assert!(!watcher1.update_if_needed());

    assert_eq!(watchable.watchers(), 2);
    assert_eq!(watchable.replace(2), 1);
    // A call to watch should not block since the value has already been sent
    watcher1.watch();
    // Peek shouldn't cause the watcher to block.
    assert_eq!(*watcher1.peek(), 2);
    watcher1.watch();
    // Reading should switch the state back to needing to block, which we'll
    // test in an other unit test
    assert_eq!(*watcher1.read(), 2);
    assert!(!watcher1.update_if_needed());
    drop(watcher1);
    assert_eq!(watchable.watchers(), 1);

    // Now, despite watcher1 having updated, watcher2 should have independent state
    assert!(watcher2.update_if_needed());
    assert_eq!(*watcher2.read(), 2);
    drop(watcher2);
    assert_eq!(watchable.watchers(), 0);
}

#[test]
fn timeouts() {
    let watchable = Watchable::new(1_u32);
    let watcher1 = watchable.subscribe();
    let start = Instant::now();
    watcher1.watch_timeout(Duration::from_secs(1));
    watcher1.watch_until(Instant::now() + Duration::from_secs(1));
    let elapsed = Instant::now().checked_duration_since(start).unwrap();
    // We don't control the delay logic, so to ensure this test is stable, we're
    // comparing against a duration slightly less than 2 seconds even though in
    // theory that shouldn't be possible.
    assert!(elapsed.as_secs_f32() >= 1.9);
}

#[test]
fn deref_publish() {
    let watchable = Watchable::new(1_u32);
    let mut watcher = watchable.subscribe();
    // Reading the value (Deref) shouldn't publish a new value
    {
        let write_guard = watchable.write();
        assert_eq!(*write_guard, 1);
    }
    assert!(!watcher.update_if_needed());
    // Writing a value (DerefMut) should publish a new value
    {
        let mut write_guard = watchable.write();
        *write_guard = 2;
    }
    assert!(watcher.update_if_needed());
}

#[test]
fn blocking_tests() {
    let watchable = Watchable::new(1_u32);
    let mut watcher = watchable.subscribe();
    let worker_thread = std::thread::spawn(move || {
        watcher.watch();
        assert_eq!(*watcher.read(), 2);
        watcher.watch();
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
    let watcher = watchable.subscribe();
    let worker_thread = std::thread::spawn(move || {
        for value in watcher {
            if value == 1000 {
                break;
            }
        }
    });

    for i in 1..=1000 {
        watchable.replace(i);
    }
    worker_thread.join().unwrap();
}

#[cfg(test)]
#[tokio::test]
async fn stream_test() {
    use futures_util::StreamExt;

    let watchable = Watchable::new(1_u32);
    let watcher = watchable.subscribe();
    let worker_thread = tokio::task::spawn(async move {
        let mut stream = watcher.into_stream();
        while let Some(value) = stream.next().await {
            if value == 1000 {
                return true;
            }
        }

        unreachable!()
    });

    for i in 1..=1000 {
        watchable.replace(i);
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    assert!(
        worker_thread.await.unwrap(),
        "Thread did not see value 1000"
    );
}

#[test]
fn stress_test() {
    let watchable = Watchable::new(1_u32);
    let mut workers = Vec::new();
    for _ in 1..=64 {
        let mut watcher = watchable.subscribe();
        workers.push(std::thread::spawn(move || {
            let mut last_value = *watcher.read();
            loop {
                watcher.watch();
                let current_value = *watcher.read();
                assert_ne!(last_value, current_value);
                if current_value == 10000 {
                    break;
                }
                last_value = current_value;
            }
        }));
    }

    for i in 1..=10000 {
        let _ = watchable.update(i);
    }

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
        let mut watcher = watchable.subscribe();
        workers.push(tokio::task::spawn(async move {
            let mut last_value = *watcher.read();
            loop {
                watcher.watch_async().await;
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
