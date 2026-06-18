//! TLAB — Thread-Local Allocation Buffer.
//!
//! Phase 2b item 6 of the metal-tier allocator redesign per
//! `docs/v0.7-A2-finding.md`. Provides a per-thread (currently
//! single-global, multi-thread upgrade in Phase 2c) cache of
//! recently-freed slots, indexed by size class, accessed via
//! cycle-cheap non-atomic push/pop. Hot path becomes:
//!
//! ```text
//! alloc(size):
//!   class_idx = SIZE_CLASSES.bucket_for(size)
//!   if let Some(p) = TLAB.pop(class_idx) { return p }   // ~3 cycles fast path
//!   fall through to CORE_ALLOC.alloc(size)              // central, slower
//!
//! free(ptr, size):
//!   class_idx = SIZE_CLASSES.bucket_for(size)
//!   if TLAB.push(class_idx, ptr) { return }             // ~3 cycles fast path
//!   fall through to CORE_ALLOC.dealloc(ptr, size)       // TLAB full
//! ```
//!
//! This matches the libc nano-allocator thread-cache cycle count
//! (the perf delta that A2 Phase 1 bench regression surfaced).
//! After Phase 2b ships + integrates, alloc-heavy benches should
//! recover to baseline-or-better.
//!
//! **Scaffolding only at this commit (item 6).** Item 7 will
//! integrate TlabCache into `core::alloc_sized` / `core::free_sized`
//! as a TLAB.pop/push fast path with refill-from-Allocator on
//! pop-miss / drain-to-Allocator on push-overflow.
//!
//! Design references (正统):
//! - mimalloc `mi_heap_t` per-page free lists with deferred frees
//! - tcmalloc `ThreadCache` size-class cache (default 32-slot)
//! - Go runtime `mcache.alloc` `*mspan` array per size class

use core::ptr;

use crate::size_class::SIZE_CLASSES;

/// Maximum cached slots per size class. Larger = fewer central-
/// fetch roundtrips, more memory held in cache. Sized per
/// tcmalloc/mimalloc defaults; tunable in Phase 2b item 7
/// integration once cycle bench data is in.
pub const TLAB_CACHE_DEPTH: usize = 16;

/// Per-thread cache of recently-freed slots, indexed by size
/// class. Total static cost: `SIZE_CLASSES.len() * TLAB_CACHE_DEPTH
/// * sizeof(*mut u8)` ≈ 1.2 KB.
///
/// `#[repr(C)]` (Phase 2e item 13 prerequisite): layout is
/// codegen-visible. The LLVM-era backend inlined TLAB.pop / push
/// at user-binary alloc/free sites using the offset constants
/// below (native ARM64 re-port = swap-3+ backlog) —
/// eliminates extern "C" call overhead, parity with libc
/// nano-allocator inline thread-cache.
#[repr(C)]
pub struct TlabCache {
    /// `slots[class][0..depth[class]]` hold cached pointers; rest
    /// is uninitialized (depth gates iteration).
    /// Layout offset = 0 (first field).
    slots: [[*mut u8; TLAB_CACHE_DEPTH]; SIZE_CLASSES.len()],
    /// Per-class occupied count.
    /// Layout offset = `TLAB_CACHE_DEPTH * SIZE_CLASSES.len() * 8`.
    depth: [u8; SIZE_CLASSES.len()],
}

// ============================================================
// Codegen layout constants — exposed for toolchain inline emit
// ============================================================

/// Byte offset of `slots` field within `TlabCache`. Equal to 0
/// (first field of #[repr(C)] struct).
pub const TLAB_SLOTS_OFFSET: usize = 0;

/// Byte offset of `depth` field within `TlabCache`. Equal to size
/// of the `slots` field = TLAB_CACHE_DEPTH * SIZE_CLASSES.len() *
/// sizeof::<*mut u8>() (8 bytes per ptr on 64-bit targets).
pub const TLAB_DEPTH_OFFSET: usize =
    TLAB_CACHE_DEPTH * SIZE_CLASSES.len() * core::mem::size_of::<*mut u8>();

/// Total size of `TlabCache` struct in bytes (rounded up to
/// alignment by Rust layout rules). Sanity-checked by
/// `ir_layout_constants_match_actual_struct` test; inline-emit
/// codegen sizes its accesses with this.
pub const TLAB_TOTAL_SIZE: usize = core::mem::size_of::<TlabCache>();

/// Per-slot stride (= bytes per cached pointer). 8 on 64-bit.
pub const TLAB_SLOT_STRIDE: usize = core::mem::size_of::<*mut u8>();

/// Per-class slots stride (= bytes per `slots[class]` row).
/// = `TLAB_CACHE_DEPTH * TLAB_SLOT_STRIDE`.
pub const TLAB_CLASS_SLOTS_STRIDE: usize = TLAB_CACHE_DEPTH * TLAB_SLOT_STRIDE;

impl Default for TlabCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TlabCache {
    pub const fn new() -> Self {
        TlabCache {
            slots: [[ptr::null_mut(); TLAB_CACHE_DEPTH]; SIZE_CLASSES.len()],
            depth: [0; SIZE_CLASSES.len()],
        }
    }

    /// Pop a cached slot for `class_idx`. Returns `None` if the
    /// TLAB has no slots cached for this class — caller falls
    /// back to the central `Allocator`. ~3 cycles when hit
    /// (single load + single subtract + single load).
    #[inline(always)]
    pub fn pop(&mut self, class_idx: usize) -> Option<*mut u8> {
        let d = self.depth[class_idx];
        if d == 0 {
            return None;
        }
        let new_depth = d - 1;
        self.depth[class_idx] = new_depth;
        Some(self.slots[class_idx][new_depth as usize])
    }

    /// Push a slot into the TLAB cache for `class_idx`. Returns
    /// `false` if the cache is full — caller must dispatch the
    /// slot back to the central `Allocator`. ~3 cycles when not
    /// full (single load + compare + single store + single store).
    #[inline(always)]
    pub fn push(&mut self, class_idx: usize, ptr: *mut u8) -> bool {
        let d = self.depth[class_idx] as usize;
        if d >= TLAB_CACHE_DEPTH {
            return false;
        }
        self.slots[class_idx][d] = ptr;
        self.depth[class_idx] = (d + 1) as u8;
        true
    }

    /// Drain all cached slots for `class_idx`, invoking `f(ptr)`
    /// for each. Used by Phase 2c TLAB destruction / cross-thread
    /// spillover and by the integration layer's "flush to
    /// central when TLAB grows too large" path.
    pub fn drain<F: FnMut(*mut u8)>(&mut self, class_idx: usize, mut f: F) {
        let d = self.depth[class_idx] as usize;
        for i in 0..d {
            f(self.slots[class_idx][i]);
        }
        self.depth[class_idx] = 0;
    }

    /// Total cached slot count across all classes. Diagnostic.
    pub fn total_cached(&self) -> usize {
        self.depth.iter().map(|&d| d as usize).sum()
    }

    /// True iff every class has zero cached slots.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.depth.iter().all(|&d| d == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_tlab_is_empty() {
        let t = TlabCache::new();
        assert!(t.is_empty());
        assert_eq!(t.total_cached(), 0);
    }

    #[test]
    fn pop_on_empty_returns_none() {
        let mut t = TlabCache::new();
        for c in 0..SIZE_CLASSES.len() {
            assert_eq!(t.pop(c), None, "class {} pop on empty", c);
        }
    }

    #[test]
    fn push_then_pop_lifo() {
        let mut t = TlabCache::new();
        let p1 = 0x1000usize as *mut u8;
        let p2 = 0x2000usize as *mut u8;
        let p3 = 0x3000usize as *mut u8;
        assert!(t.push(2, p1));
        assert!(t.push(2, p2));
        assert!(t.push(2, p3));
        assert_eq!(t.total_cached(), 3);
        assert_eq!(t.pop(2), Some(p3));
        assert_eq!(t.pop(2), Some(p2));
        assert_eq!(t.pop(2), Some(p1));
        assert_eq!(t.pop(2), None);
        assert!(t.is_empty());
    }

    #[test]
    fn push_until_full_returns_false() {
        let mut t = TlabCache::new();
        for i in 0..TLAB_CACHE_DEPTH {
            assert!(t.push(0, (i + 1) as *mut u8), "push {} fits", i);
        }
        // Cache full → push should return false; depth unchanged.
        assert!(
            !t.push(0, 0xdead as *mut u8),
            "overflow push must return false"
        );
        assert_eq!(t.total_cached(), TLAB_CACHE_DEPTH);
    }

    #[test]
    fn classes_are_independent() {
        let mut t = TlabCache::new();
        assert!(t.push(0, 0x100 as *mut u8));
        assert!(t.push(5, 0x500 as *mut u8));
        assert!(t.push(8, 0x800 as *mut u8));
        // Pop from each class — each independent LIFO.
        assert_eq!(t.pop(0), Some(0x100 as *mut u8));
        assert_eq!(t.pop(5), Some(0x500 as *mut u8));
        assert_eq!(t.pop(8), Some(0x800 as *mut u8));
        assert!(t.is_empty());
    }

    #[test]
    fn drain_iterates_all_in_class() {
        let mut t = TlabCache::new();
        let ptrs = [0x10, 0x20, 0x30, 0x40].map(|n| n as *mut u8);
        for p in ptrs.iter() {
            assert!(t.push(3, *p));
        }
        let mut seen = vec![];
        t.drain(3, |p| seen.push(p));
        assert_eq!(seen.len(), 4);
        // drain doesn't preserve order; assert set membership.
        for p in ptrs.iter() {
            assert!(seen.contains(p));
        }
        // Class is empty after drain.
        assert_eq!(t.pop(3), None);
    }

    #[test]
    fn drain_empty_class_is_noop() {
        let mut t = TlabCache::new();
        let mut count = 0;
        t.drain(0, |_| count += 1);
        assert_eq!(count, 0);
    }

    /// Pin TlabCache layout for toolchain inline emit (Phase 2e).
    /// If these constants drift from actual struct layout, emitted
    /// TLAB.pop / push reads/writes wrong fields → memory corruption.
    /// Test catches drift at `cargo test` time before any user binary
    /// is built with the stale offsets baked in.
    #[test]
    fn ir_layout_constants_match_actual_struct() {
        let t = TlabCache::new();
        let base = &t as *const _ as usize;
        let slots_addr = &t.slots as *const _ as usize;
        let depth_addr = &t.depth as *const _ as usize;
        assert_eq!(
            slots_addr - base,
            TLAB_SLOTS_OFFSET,
            "TLAB_SLOTS_OFFSET drift"
        );
        assert_eq!(
            depth_addr - base,
            TLAB_DEPTH_OFFSET,
            "TLAB_DEPTH_OFFSET drift"
        );
        assert_eq!(
            core::mem::size_of::<TlabCache>(),
            TLAB_TOTAL_SIZE,
            "TLAB_TOTAL_SIZE drift"
        );
        // Sanity: depth offset should equal slots field size.
        let expected_depth_off = SIZE_CLASSES.len() * TLAB_CACHE_DEPTH * 8;
        assert_eq!(TLAB_DEPTH_OFFSET, expected_depth_off);
    }
}
