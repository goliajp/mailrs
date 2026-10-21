# Changelog

All notable changes to `mailrs-mmalloc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `LICENSE-APACHE` and `LICENSE-MIT` are now shipped with the crate.

## [0.1.0] - 2026-08-04

### Added

- Initial release. An mmap-backed allocator for Linux x86_64 and
  aarch64. `MailrsAllocator` implements `GlobalAlloc` and can be
  installed with `#[global_allocator]`.
- Layers: per-thread caches (`tlab`, `thread_cache`), 32 size classes up
  to 4096 bytes served from 512 KiB spans (`size_class`, `span`), a
  cross-thread free queue (`central`), and direct `mmap` for larger
  requests (`large`).
- Empty spans release their pages to the operating system with
  `madvise(MADV_DONTNEED)` / `munmap`, so resident memory goes down
  after a burst.
- `core::stats()` returns `AllocatorStats`.
- On targets other than Linux, `MailrsAllocator` delegates to
  `std::alloc::System`.
