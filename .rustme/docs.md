Allows watching for value changes in both multi-threaded and asynchronous
contexts.

`watchable` is a Multi-Producer, Multi-Consumer channel where each consumer
is only guaranteed to receive the most recently written value.

```rust
$../examples/simple.rs$
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

As you can see, the receiving thread doesn't receive every value. Each sentinel
is guaranteed to be notified when changes occur and is guaranteed to be able to
retrieve the most recent value.

## Async Support

The `Sentinel` type can be used in async code in multiple ways:

- `Sentinel::into_stream()`: Wraps the sentinel in a type that implements
  `futures::Stream`.
- `Sentinel::wait_async().await`: Pauses execution of the current task until a
  new value is available to be read. `Sentinel::read()` can be used to retrieve
  the current value after `wait_async()` has returned.

Here is the same example as above, except this time using `Sentinel::into_stream` with Tokio:

```rust
$../examples/simple-async.rs$
```
