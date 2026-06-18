//! `#[global_allocator]` wiring — routes Rust std/alloc crate's
//! `__rust_alloc` / `__rust_dealloc` / `__rust_realloc` /
//! `__rust_alloc_zeroed` shim symbols to the mailrs-mmalloc
//! Layer 1 sized API (`core::alloc_sized` / `core::free_sized`).
//!
//! Without this, every Rust `Box::new` / `Vec::push` / `String::from`
//! inside the Layer-1+ staticlib chain falls back to
//! `std::alloc::System` → libc `_malloc` / `_free` / `_realloc` /
//! `_posix_memalign`. v0.7-A5 16-d makes those four symbols
//! disappear from the user binary's `nm` undef list — Rust std now
//! uses our mmap-backed allocator end-to-end.
//!
//! ## Alignment contract
//!
//! `core::alloc_sized` guarantees 16-byte alignment for small
//! allocations (size_class path) and 4096-byte page alignment for
//! large allocations (mmap-direct path). Any `Layout::align()`
//! ≤ 16 is satisfied directly. For 16 < align ≤ usize::MAX we
//! over-allocate `size + align - 1 + sizeof(usize)`, store the
//! original base pointer in a pre-header, and return an aligned
//! pointer inside the over-allocated region. `dealloc` reads the
//! pre-header to recover the base + uses the original Layout to
//! recover the total allocation size.
//!
//! ## Why not a stand-alone crate
//!
//! `#[global_allocator]` must be linked into the user binary's
//! staticlib chain. mailrs-mmalloc is already in `TORAJS_STATICLIBS`
//! (`libmailrs_mmalloc.a`), so co-locating the GlobalAlloc impl
//! avoids adding a separate `torajs-alloc-glue` crate that would
//! just re-export this one type.

use core::alloc::{GlobalAlloc, Layout};
use core::mem::size_of;

use crate::core::{alloc_sized, free_sized};

/// Per-allocation pre-header storing the original base pointer for
/// over-allocated (align > 16) blocks. One `usize` (8 bytes on
/// 64-bit) at `aligned_ptr - sizeof(usize)`.
const HEADER: usize = size_of::<usize>();

/// Threshold above which we fall back to over-allocation. The
/// underlying size_class allocator guarantees ≥ 16-byte alignment;
/// anything stricter needs the over-alloc + pre-header path.
const NATIVE_ALIGN: usize = 16;

/// Zero-sized marker type that owns the `#[global_allocator]`
/// dispatch contract. All state lives in the global `CORE_*`
/// statics inside `core.rs`.
pub struct MailrsAllocator;

unsafe impl GlobalAlloc for MailrsAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        if align <= NATIVE_ALIGN {
            return alloc_sized(size);
        }
        let total = match over_alloc_total(size, align) {
            Some(n) => n,
            None => return core::ptr::null_mut(),
        };
        let raw = alloc_sized(total);
        if raw.is_null() {
            return raw;
        }
        let aligned = align_up_with_header(raw as usize, align);
        unsafe {
            core::ptr::write((aligned - HEADER) as *mut usize, raw as usize);
        }
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let align = layout.align();
        if align <= NATIVE_ALIGN {
            unsafe { free_sized(ptr, size) };
            return;
        }
        let raw = unsafe { core::ptr::read((ptr as usize - HEADER) as *const usize) } as *mut u8;
        let total = match over_alloc_total(size, align) {
            Some(n) => n,
            None => return,
        };
        unsafe { free_sized(raw, total) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.alloc(layout) };
        if !p.is_null() {
            unsafe { core::ptr::write_bytes(p, 0, layout.size()) };
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(l) => l,
            Err(_) => return core::ptr::null_mut(),
        };
        let new_ptr = unsafe { self.alloc(new_layout) };
        if new_ptr.is_null() {
            return new_ptr;
        }
        let copy_len = core::cmp::min(layout.size(), new_size);
        unsafe { core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_len) };
        unsafe { self.dealloc(ptr, layout) };
        new_ptr
    }
}

/// Total bytes to over-allocate for an `align > 16` request.
/// Returns `None` on `usize` overflow.
fn over_alloc_total(size: usize, align: usize) -> Option<usize> {
    size.checked_add(align - 1)?.checked_add(HEADER)
}

/// Compute the aligned pointer inside an over-allocated block, with
/// `HEADER` bytes reserved before it for the original-base record.
fn align_up_with_header(raw: usize, align: usize) -> usize {
    let after_hdr = raw + HEADER;
    (after_hdr + align - 1) & !(align - 1)
}

// Note: `#[global_allocator] static GLOBAL: MailrsAllocator` is
// **not** declared here. Rust emits the `__rust_alloc_*` shim
// inside every staticlib that holds the marker; declaring it in
// mmalloc alongside the default fallback in `torajs-panic-runtime`
// (which the LLVM-era link step force-loaded first; torajs-link
// resolves it through the archive worklist now) collides
// at user-binary link time. The single-marker site is
// `torajs-panic-runtime` active mode — it `extern "C"`-routes the
// shim to `__torajs_libc_malloc / _free / _realloc` exported here,
// keeping the GlobalAlloc trait math in this file while the link-
// level binding stays on the force_load-priority staticlib.

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: alignment-aware over-alloc math doesn't overflow on
    /// modest size + align combinations.
    #[test]
    fn over_alloc_total_basic() {
        assert_eq!(over_alloc_total(64, 64), Some(64 + 63 + HEADER));
        assert_eq!(over_alloc_total(1024, 256), Some(1024 + 255 + HEADER));
    }

    /// Saturating safety on `usize::MAX - 1` size with align 64.
    #[test]
    fn over_alloc_total_overflow_returns_none() {
        assert_eq!(over_alloc_total(usize::MAX, 64), None);
    }

    /// `align_up_with_header` puts the returned ptr at a multiple
    /// of `align` AND leaves space for the HEADER before it.
    #[test]
    fn align_up_with_header_satisfies_align() {
        for &align in &[32usize, 64, 128, 256, 512] {
            for offset in 0..align {
                let raw = 0x1000 + offset;
                let aligned = align_up_with_header(raw, align);
                assert!(aligned % align == 0);
                assert!(aligned >= raw + HEADER);
            }
        }
    }

    /// The pre-header offset is enough room for one `usize`.
    #[test]
    fn header_size_is_one_usize() {
        assert_eq!(HEADER, size_of::<usize>());
    }

    /// The `alloc / dealloc` round-trip works for align ≤ 16 (Layer
    /// 1 fast path) — the GlobalAlloc impl just routes to
    /// `alloc_sized` / `free_sized`. Test exercises a few common
    /// `Box<T>` and `Vec<T>` shape allocations the Rust std would
    /// emit.
    #[test]
    fn alloc_dealloc_small_align() {
        let a = MailrsAllocator;
        for &(size, align) in &[(8usize, 8usize), (16, 8), (24, 8), (64, 8), (128, 16)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = unsafe { a.alloc(layout) };
            assert!(!p.is_null(), "alloc({size}, {align}) returned null");
            assert_eq!(p as usize % align, 0);
            unsafe { core::ptr::write_bytes(p, 0xaa, size) };
            unsafe { a.dealloc(p, layout) };
        }
    }

    /// The over-alloc + pre-header path satisfies align > 16
    /// requests. Verifies the returned ptr is aligned AND that
    /// dealloc round-trips without crashing.
    #[test]
    fn alloc_dealloc_over_alloc_path() {
        let a = MailrsAllocator;
        for &(size, align) in &[(64usize, 32usize), (128, 64), (256, 128), (512, 256)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = unsafe { a.alloc(layout) };
            assert!(!p.is_null(), "alloc({size}, {align}) returned null");
            assert_eq!(p as usize % align, 0, "over-alloc ptr not aligned");
            unsafe { core::ptr::write_bytes(p, 0xbb, size) };
            unsafe { a.dealloc(p, layout) };
        }
    }
}
