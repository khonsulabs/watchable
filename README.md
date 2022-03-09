# Watchable

Allows watching for value changes in both multi-threaded and asynchronous
contexts.

![watchable forbids unsafe code](https://img.shields.io/badge/unsafe-forbid-success)
[![crate version](https://img.shields.io/crates/v/watchable.svg)](https://crates.io/crates/watchable)
[![Live Build Status](https://img.shields.io/github/workflow/status/khonsulabs/watchable/Tests/main)](https://github.com/khonsulabs/watchable/actions?query=workflow:Tests)
[![HTML Coverage Report for `main` branch](https://khonsulabs.github.io/watchable/coverage/badge.svg)](https://khonsulabs.github.io/watchable/coverage/)
[![Documentation for `main` branch](https://img.shields.io/badge/docs-main-informational)](https://khonsulabs.github.io/watchable/main/watchable/)

`watchable` is a Multi-Producer, Multi-Consumer channel where each consumer
is only guaranteed to receive the most recently written value.

```rust
use watchable::{Watchable, Watcher};

fn main() {
    // Create a Watchable<u32> which holds a u32 and notifies watchers when the
    // contained value changes.
    let watchable = Watchable::default();
    // Create a watcher that will efficiently be able to monitor and read the
    // contained value as it is updated.
    let watcher = watchable.watch();
    // Spawn a background worker that will print out the values the watcher reads.
    let watching_thread = std::thread::spawn(|| watching_thread(watcher));

    // Store a sequence of values. Each time a new value is written, any waiting
    // watchers will be notified there is a new value available.
    for i in 1_u32..=1000 {
        watchable.replace(i);
    }

    // Once we're done sending values, dropping the Watchable will ensure
    // watchers are notified of the disconnection. Watchers are guaranteed to be
    // able to read the final value.
    drop(watchable);

    // Wait for the thread to exit.
    watching_thread.join().unwrap();
}

fn watching_thread(watcher: Watcher<u32>) {
    // A Watcher can be used as an iterator which always reads the most
    // recent value, or parks the current thread until a new value is available.
    for value in watcher {
        // The value we read will not necessarily be sequential, even though the
        // main thread is storing a complete sequence.
        println!("Read value: {value}");
    }
}
```

When running this example, the output will look similar to:

```sh
...
Read value: 876
Read value: 897
Read value: 923
Read value: 944
Read value: 957
Read value: 977
Read value: 995
Read value: 1000
```

As you can see, the receiving thread doesn't receive every value. Each watcher
is guaranteed to be notified when changes occur and is guaranteed to be able to
retrieve the most recent value.

## Async Support

The `Watcher` type can be used in async code in multiple ways:

- `Watcher::into_stream()`: Wraps the watcher in a type that implements
  `futures::Stream`.
- `Watcher::wait_async().await`: Pauses execution of the current task until a
  new value is available to be read. `Watcher::read()` can be used to retrieve
  the current value after `wait_async()` has returned.

Here is the same example as above, except this time using `Watcher::into_stream` with Tokio:

```rust
use futures_util::StreamExt;
use watchable::{Watchable, Watcher};

#[tokio::main]
async fn main() {
    // Create a Watchable<u32> which holds a u32 and notifies watchers when the
    // contained value changes.
    let watchable = Watchable::default();
    // Create a watcher that will efficiently be able to monitor and read the
    // contained value as it is updated.
    let watcher = watchable.watch();
    // Spawn a background worker that will print out the values the watcher reads.
    let watching_task = tokio::task::spawn(watching_task(watcher));

    // Store a sequence of values. Each time a new value is written, any waiting
    // watchers will be notified there is a new value available.
    for i in 1_u32..=1000 {
        watchable.replace(i);
    }

    // Once we're done sending values, dropping the Watchable will ensure
    // watchers are notified of the disconnection. Watchers are guaranteed to be
    // able to read the final value.
    drop(watchable);

    // Wait for the spawned task to exit.
    watching_task.await.unwrap();
}

async fn watching_task(watcher: Watcher<u32>) {
    // A Watcher can be converted into a Stream, which allows for asynchronous
    // iteration.
    let mut stream = watcher.into_stream();
    while let Some(value) = stream.next().await {
        // The value we received will not necessarily be sequential, even though
        // the main thread is publishing a complete sequence.
        println!("Read value: {value}");
    }
}
```

`watchable` is compatible with all async runtimes.

## Open-source Licenses

This project, like all projects from [Khonsu Labs](https://khonsulabs.com/), are
open-source. This repository is available under the [MIT License](./LICENSE-MIT)
or the [Apache License 2.0](./LICENSE-APACHE).

To learn more about contributing, please see [CONTRIBUTING.md](./CONTRIBUTING.md).
