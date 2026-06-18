//! Non-Linux host stub for `MailrsAllocator`.
//!
//! On macOS (or any non-Linux dev target), the mmap-backed
//! allocator path is unreachable because the underlying
//! `mailrs_syscall::mmap_anon_rw` is a no-op stub there. Provide a
//! `MailrsAllocator` shape that delegates to `std::alloc::System`
//! so a developer can still wire `#[global_allocator] = ...` in
//! `crates/server/src/main.rs` without making the macOS host build
//! unbuildable. The Linux production binary uses the real mmap
//! allocator.

use core::alloc::{GlobalAlloc, Layout};

/// `#[global_allocator]`-compatible marker. On non-Linux hosts this
/// just forwards every call to `std::alloc::System` — equivalent to
/// having no override at all, but it lets the same `static ALLOC:
/// MailrsAllocator = MailrsAllocator;` declaration sit in
/// `main.rs` regardless of host OS.
pub struct MailrsAllocator;

unsafe impl GlobalAlloc for MailrsAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}
