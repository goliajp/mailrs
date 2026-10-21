# mailrs-mmalloc

[![Crates.io](https://img.shields.io/crates/v/mailrs-mmalloc?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-mmalloc)
[![docs.rs](https://img.shields.io/docsrs/mailrs-mmalloc?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-mmalloc)
[![License](https://img.shields.io/crates/l/mailrs-mmalloc?style=flat-square)](#license)

An mmap-backed global allocator for Linux on x86_64 and aarch64. All
memory comes from `mmap` through
[`mailrs-syscall`](https://crates.io/crates/mailrs-syscall); there is no
`brk` and no libc `malloc` arena. When a span empties, its pages are
returned to the operating system with `madvise(MADV_DONTNEED)`, so the
resident set shrinks after a burst of allocations instead of staying at
its peak, which is what glibc's per-thread arenas tend to do in a
long-running multi-threaded server.

The layered design follows `torajs-mmalloc` from
[goliajp/torajs](https://github.com/goliajp/torajs), without its C ABI
surface.

## Usage

```rust,ignore
#[global_allocator]
static ALLOC: mailrs_mmalloc::MailrsAllocator = mailrs_mmalloc::MailrsAllocator;
```

On every other target `MailrsAllocator` forwards to
`std::alloc::System`, so the same code builds and runs on a macOS
development machine.

## Layers

1. **Thread-local cache** (`tlab`, `thread_cache`): recently freed slots
   per size class, the fastest path for small allocations.
2. **Size classes** (`size_class`, `span`): 32 classes from 16 to 4096
   bytes, each served from 512 KiB spans with a free list.
3. **Cross-thread frees** (`central`): a lock-free queue for slots freed
   by a thread that does not own them.
4. **Large allocations** (`large`): requests over 4096 bytes get their
   own page-aligned `mmap` and are `munmap`ped on free.

`core::stats()` reports mapped, resident and in-use bytes, allocation
counts, outstanding large allocations and per-class figures. Measured
costs and the regression budgets are in `BUDGETS.md`.

## License

Apache-2.0 OR MIT.
