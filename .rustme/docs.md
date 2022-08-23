`watchable` implements an observable RwLock-like type that is compatible with
both multi-threaded and async code. Inspired by
[tokio::sync::watch][tokio-watch].

![watchable forbids unsafe code](https://img.shields.io/badge/unsafe-forbid-success)
[![crate version](https://img.shields.io/crates/v/watchable.svg)](https://crates.io/crates/watchable)
[![Live Build Status](https://img.shields.io/github/workflow/status/khonsulabs/watchable/Tests/main)](https://github.com/khonsulabs/watchable/actions?query=workflow:Tests)
[![HTML Coverage Report for `main` branch](https://khonsulabs.github.io/watchable/coverage/badge.svg)](https://khonsulabs.github.io/watchable/coverage/)
[![Documentation for `main` branch](https://img.shields.io/badge/docs-main-informational)]($docs$)

`watchable` is an RwLock-like type that allows watching for value changes
using a Multi-Producer, Multi-Consumer approach where each consumer is only
guaranteed to receive the most recently written value.

```rust
$../examples/simple.rs:example$
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
$../examples/simple-async.rs:example$
```

`watchable` is compatible with all async runtimes.

[tokio-watch]: https://docs.rs/tokio/latest/tokio/sync/watch/index.html
