# Changelog

All notable changes to `mailrs-syscall` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The `LICENSE-APACHE` and `LICENSE-MIT` texts are shipped with the
  crate. No code change.

## [0.1.0] - 2026-08-04

### Added

- Initial release. Raw Linux system calls without libc, for x86_64 and
  aarch64: `mmap_anon_rw`, `mmap_anon_rw_aligned`, `munmap`,
  `madvise_dontneed` and `gettid`, with `Errno` for failures.
- Stubs on non-Linux targets: the memory functions return `ENOSYS` and
  `gettid` returns `0`.
