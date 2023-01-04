# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## v1.1.0

### Changes

- `Watchable::shutdown()` has been added to disconnect all `Watcher`s without
  needing to drop all instances of `Watchable`. Any existing value changes will
  still be observed by `Watcher`s before they return a disconnected error on the
  next attempt to read a value.

## v1.0.0

- Initial stable release.
