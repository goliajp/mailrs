//! Large-allocation fallback — direct `mmap` per request when the
//! size exceeds the largest size-class bucket (4096 bytes).
//!
//! No pooling; large allocs are assumed infrequent enough that
//! per-request `mmap` + `munmap` cost is acceptable. Each block is
//! page-rounded up so munmap with the same (addr, len) is valid.

use mailrs_syscall::{Errno, mmap_anon_rw, munmap};

/// Kernel page size on macOS aarch64. (kernel uses 16 KB internally
/// for VM but mmap accepts any 4 KB multiple — keep the math simple.)
pub const PAGE_4K: usize = 4096;

/// Round `n` up to the next 4 KB boundary.
#[inline]
pub fn page_round_up(n: usize) -> usize {
    (n + PAGE_4K - 1) & !(PAGE_4K - 1)
}

/// Allocate `size` bytes via direct `mmap`. `size` is rounded up to
/// a 4 KB page boundary internally; caller passes the original
/// `size` to `large_free` (which will round-up again to match).
pub fn large_alloc(size: usize) -> Result<*mut u8, Errno> {
    let rounded = page_round_up(size.max(1));
    mmap_anon_rw(rounded)
}

/// Release a block returned by `large_alloc`. `size` must be the
/// original argument passed to `large_alloc` (NOT the rounded
/// size — the function applies the same round-up internally).
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a prior matching
/// `large_alloc(size)` call.
pub unsafe fn large_free(ptr: *mut u8, size: usize) -> Result<(), Errno> {
    let rounded = page_round_up(size.max(1));
    unsafe { munmap(ptr, rounded) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn large_alloc_roundtrip() {
        let size = 8192;
        let p = large_alloc(size).expect("large_alloc");
        unsafe {
            for i in 0..size {
                ptr::write(p.add(i), (i & 0xff) as u8);
            }
            for i in 0..size {
                assert_eq!(*p.add(i), (i & 0xff) as u8);
            }
        }
        unsafe { large_free(p, size) }.expect("large_free");
    }

    #[test]
    fn page_round_up_aligns_to_4k() {
        assert_eq!(page_round_up(1), 4096);
        assert_eq!(page_round_up(4096), 4096);
        assert_eq!(page_round_up(4097), 8192);
        assert_eq!(page_round_up(12345), 16384);
    }
}
