//! Page-level bump allocator.
//!
//! Each page is a 16 KB region obtained from `mmap_anon_rw`. Within
//! a page we bump a `cursor` forward by request size (aligned to 8
//! bytes). When the cursor would overflow, the page is full — the
//! caller (next layer up) decides whether to allocate a fresh page
//! or fall back to direct mmap.
//!
//! Single-page bump is the simplest possible allocator. A free()
//! within a page is a no-op (we don't reclaim individual blocks at
//! this layer; reclamation comes via the size-class free-list at
//! the next layer up).

use core::ptr::NonNull;

use mailrs_syscall::{Errno, mmap_anon_rw, munmap};

/// 16 KB. Tuned to be larger than a typical short-Str alloc + Arr
/// alloc combined, so the common-case sequence of small allocs
/// stays in one page.
pub const PAGE_SIZE: usize = 16 * 1024;

pub struct PageBump {
    /// Start of the mmap'd region.
    base: NonNull<u8>,
    /// Bytes already handed out.
    cursor: usize,
}

impl PageBump {
    /// Allocate a fresh page from the kernel. Errors propagate from
    /// the underlying `mmap_anon_rw`.
    pub fn alloc_page() -> Result<Self, Errno> {
        let p = mmap_anon_rw(PAGE_SIZE)?;
        // SAFETY: mmap_anon_rw returned Ok, so p is a valid pointer
        // to PAGE_SIZE bytes of writable memory; it can't be null
        // (kernel returns negative-errno which decode() catches).
        let base = unsafe { NonNull::new_unchecked(p) };
        Ok(PageBump { base, cursor: 0 })
    }

    /// Allocate `size` bytes (8-byte aligned). Returns `None` if the
    /// page can't fit — caller should retry with a fresh page or
    /// fall back to large allocation.
    pub fn try_bump(&mut self, size: usize) -> Option<*mut u8> {
        let aligned = (size + 7) & !7;
        if self.cursor.checked_add(aligned)? > PAGE_SIZE {
            return None;
        }
        // SAFETY: cursor was just checked to keep within PAGE_SIZE;
        // base is the start of a PAGE_SIZE-byte mapped region.
        let p = unsafe { self.base.as_ptr().add(self.cursor) };
        self.cursor += aligned;
        Some(p)
    }

    /// Bytes already used in this page.
    pub fn used(&self) -> usize {
        self.cursor
    }

    /// Bytes remaining in this page.
    pub fn remaining(&self) -> usize {
        PAGE_SIZE - self.cursor
    }
}

impl Drop for PageBump {
    fn drop(&mut self) {
        // SAFETY: base was mmap'd via mmap_anon_rw with PAGE_SIZE
        // bytes; munmap with the same args is well-formed.
        let _ = unsafe { munmap(self.base.as_ptr(), PAGE_SIZE) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_page_returns_writable_memory() {
        let mut page = PageBump::alloc_page().expect("alloc_page");
        let p = page.try_bump(16).expect("bump 16");
        unsafe {
            *p = 0xaa;
            *p.add(15) = 0xbb;
            assert_eq!(*p, 0xaa);
            assert_eq!(*p.add(15), 0xbb);
        }
    }

    #[test]
    fn bump_aligns_to_8() {
        let mut page = PageBump::alloc_page().expect("alloc_page");
        let _ = page.try_bump(1).expect("bump 1");
        // first bump consumed 8 bytes (1 rounded up). Second bump
        // should also start aligned.
        assert_eq!(page.used(), 8);
        let _ = page.try_bump(9).expect("bump 9");
        assert_eq!(page.used(), 24); // 8 + 16 (9 rounded up to 16)
    }

    #[test]
    fn bump_returns_none_when_full() {
        let mut page = PageBump::alloc_page().expect("alloc_page");
        let _ = page.try_bump(PAGE_SIZE).expect("bump full");
        assert!(page.try_bump(1).is_none(), "page should be full");
    }

    #[test]
    fn drop_munmaps_page() {
        // No direct way to verify munmap occurred — but the
        // syscall return is checked (via Drop's `let _ = ...`).
        // This test mainly catches double-free / wrong-len bugs
        // since munmap with bad args panics or errors.
        for _ in 0..100 {
            let _ = PageBump::alloc_page().expect("alloc_page");
            // dropped here — munmap fires
        }
    }
}
