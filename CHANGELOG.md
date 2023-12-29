# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v1.1.2

### Changed

- `Watcher<T>` now uses interior mutability via `AtomicUsize`. This allows a
  watcher to be stored inside of a static variable and be used between multiple
  threads without involving a mutex.
- This package's MSRV was incorrectly set. It has been updated to 1.64, which
  was the actual MSRV of 1.1.1

### Fixes

- `Watcher<T>` no longer requires `T` to be `Clone` for itself to be cloneable.
- `.crate-docs.md` is now included in the released package.

## v1.1.1

### Fixes

- Fixed a race condition that could cause a replace operation to be ignored if
  it happened during a short region of the Watcher's next_value logic.
  Subsequent updates would still be received.

## v1.1.0

### Changes

- `Watchable::shutdown()` has been added to disconnect all `Watcher`s without
  needing to drop all instances of `Watchable`. Any existing value changes will
  still be observed by `Watcher`s before they return a disconnected error on the
  next attempt to read a value.

## v1.0.0

- Initial stable release.
