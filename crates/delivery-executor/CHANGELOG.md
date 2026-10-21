# Changelog

All notable changes to `mailrs-delivery-executor` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.0] - 2026-05-29

### Changed

- A batch that targets several Maildir paths now flushes each path on
  its own thread (`std::thread::scope`), so the per-path fsyncs run in
  parallel instead of one after another. A batch with a single path
  takes the same code path as before. The public API is unchanged.
- The `mailrs-maildir` requirement is now `1.2`.

### Added

- `BUDGETS.md`, a criterion bench and `tests/perf_gate.rs` regression
  gates.

## [1.1.1] - 2026-05-24

### Changed

- Test coverage only (default `spawn()` path, executor shutdown when
  the last sender is dropped). No library changes.

## [1.1.0] - 2026-05-24

### Added

- `DEFAULT_MAX_CONCURRENT_FLUSHES` (2) and
  `DeliveryExecutor::with_full_config(max_batch, max_wait,
  max_concurrent_flushes)`.

### Changed

- Flushes are pipelined: while one batch is being fsynced on a
  blocking thread, the next batch is collected. The number of flushes
  in flight is bounded by a semaphore (default 2). Pass `1` to
  `with_full_config` for the strictly serial 1.0 behaviour.

## [1.0.0] - 2026-05-24

### Added

- Initial release. `DeliveryExecutor` collects delivery requests from
  concurrent async tasks and writes them to Maildir with
  `mailrs-maildir`'s `deliver_batch`, one fsync per batch instead of
  one per message.
- `DeliveryExecutor::spawn()` with defaults `DEFAULT_MAX_BATCH` (64)
  and `DEFAULT_MAX_WAIT` (10 ms); `DeliveryExecutor::with_config(max_batch,
  max_wait)`.
- `DeliveryExecutor::deliver(path, body)` resolves with the delivered
  message's id once its batch is durable.
