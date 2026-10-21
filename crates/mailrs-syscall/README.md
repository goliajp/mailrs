# mailrs-syscall

[![Crates.io](https://img.shields.io/crates/v/mailrs-syscall?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-syscall)
[![docs.rs](https://img.shields.io/docsrs/mailrs-syscall?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-syscall)
[![License](https://img.shields.io/crates/l/mailrs-syscall?style=flat-square)](#license)

Raw Linux system calls for a memory allocator, issued without libc and
with no dependencies: `mmap`, `munmap`, `madvise(MADV_DONTNEED)` and
`gettid`. It exists for `mailrs-mmalloc`, which runs inside
`#[global_allocator]`, where calling into libc during deallocation can
deadlock.

`madvise(MADV_DONTNEED)` is the call that matters: it hands the pages
of a freed region back to the kernel while the mapping stays in place,
so the process's resident memory actually drops.

## Targets

Linux on x86_64 and aarch64. On other operating systems every function
is a stub: the memory functions return `Err(Errno(38))` (`ENOSYS`) and
`gettid` returns `0`, so a workspace that depends on this crate still
builds and tests on a non-Linux development machine. Other Linux
architectures are not supported.

## API

- `mmap_anon_rw(len)`: anonymous, private, read-write mapping of `len`
  zero-filled bytes; the kernel picks the address.
- `mmap_anon_rw_aligned(len, align)`: the same, aligned to `align` (a
  power of two, at least 4096; otherwise `Errno(22)`, `EINVAL`).
  Implemented by over-allocating and unmapping the unaligned head and
  tail.
- `unsafe munmap(addr, len)`: release a mapping made by one of the
  above.
- `unsafe madvise_dontneed(addr, len)`: return the pages of a mapped
  region to the kernel; the next access sees fresh zero pages.
- `gettid()`: the calling thread's kernel thread id.
- `Errno`: the positive Linux error code of a failed call.

## License

Apache-2.0 OR MIT.
