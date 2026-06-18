// Lint diff between torajs and mailrs workspaces. We keep the
// fork's code shape verbatim so future syncs from torajs upstream
// stay a clean diff; the rules below are stylistic, not correctness.
#![allow(
    clippy::vec_box,
    clippy::manual_is_multiple_of,
    clippy::collapsible_if,
    clippy::borrow_deref_ref,
    clippy::declare_interior_mutable_const,
    clippy::large_const_arrays,
    clippy::deref_addrof
)]

//! mailrs-mmalloc — mmap-backed allocator for `mailrs-server`.
//!
//! Forked from the `goliajp/torajs` `torajs-mmalloc` v0.7 layered
//! design (commit-fingerprint preserved in the per-module module
//! docs). The torajs project's `metal-level` allocator is shaped
//! around an AOT TypeScript runtime — single-process, single
//! linker chain, custom C-ABI `__torajs_*` exports — so we drop
//! the C-ABI surface (`extern_api`) and keep only what
//! `#[global_allocator]` needs to back a tokio-multi-worker
//! Rust binary on Linux.
//!
//! All memory comes from `mmap`
//! (via [`mailrs_syscall::mmap_anon_rw`]) — no `brk` / no `libc`
//! `malloc` arena. Freed pages are returned to the OS via
//! `madvise(MADV_DONTNEED)` rather than retained in a per-thread
//! arena, which is the proximate fix for the RSS climb that
//! mailrs-server bleeds under glibc malloc (see
//! `.claude/notes/rss-leak-attribution-allocator-2026-06-18.md`).
//!
//! ## Layered structure
//!
//! 1. **Page bump** ([`page::PageBump`]) — fixed-size page (16 KB)
//!    allocated via mmap; sub-allocations bump-allocate from it
//!    until full, then a new page is requested. Fastest path for
//!    small temporaries; no per-allocation header.
//! 2. **Size-class free list** ([`size_class::SizeClassPool`]) —
//!    one LIFO free-list per power-of-two bucket
//!    (16/32/64/128/256/512/1024/2048/4096). Recycles freed blocks.
//! 3. **Direct mmap fallback** ([`large::large_alloc`]) — for
//!    `size > 4096` bytes, mmap a fresh page-aligned region and
//!    return it. `large_free` munmaps. No pooling; large allocs
//!    are assumed infrequent.
//! 4. **TLAB** ([`tlab::TlabCache`]) — thread-local last-freed
//!    block cache, hottest small-alloc path.
//!
//! ## `#[global_allocator]` wiring
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: mailrs_mmalloc::MailrsAllocator =
//!     mailrs_mmalloc::MailrsAllocator;
//! ```
//!
//! When `target_os = "linux"`, this routes every `Box::new` /
//! `Vec::push` / `String::from` through the mmap-backed allocator.
//! On non-Linux dev hosts (macOS) it falls back to a thin shim
//! that delegates to `std::alloc::System`, so cargo workspace
//! builds and tests stay green on a dev laptop.

// The real layered allocator is `target_os = "linux"` only — every
// page comes from `mailrs_syscall::mmap_anon_rw`, which is a stub
// (ENOSYS) on macOS hosts to keep cargo workspace builds green
// during dev. Activating the mmap-backed code path on macOS would
// stack-overflow on first allocation, so we hide it behind a cfg
// gate and provide a no-op `MailrsAllocator` (delegates to
// `std::alloc::System`) for non-Linux builds.
#[cfg(target_os = "linux")]
pub mod central;
#[cfg(target_os = "linux")]
pub mod core;
#[cfg(target_os = "linux")]
pub mod global_alloc;
#[cfg(target_os = "linux")]
pub mod large;
#[cfg(target_os = "linux")]
pub mod page;
#[cfg(target_os = "linux")]
pub mod size_class;
#[cfg(target_os = "linux")]
pub mod span;
#[cfg(target_os = "linux")]
pub mod tlab;

#[cfg(target_os = "linux")]
pub use global_alloc::MailrsAllocator;
#[cfg(target_os = "linux")]
pub use size_class::Allocator;
#[cfg(target_os = "linux")]
pub use span::Span;

#[cfg(not(target_os = "linux"))]
mod host_stub;
#[cfg(not(target_os = "linux"))]
pub use host_stub::MailrsAllocator;
