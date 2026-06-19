//! Size-class allocator built on [`Span`].
//!
//! Phase 2a item 2 of the metal-tier redesign — span-backed
//! replacement for the prior PageBump + per-class freelist shape.
//! Each size class owns a bounded array of spans
//! (`[Option<Span>; PER_CLASS_CAP]`). Allocation walks spans LIFO
//! (most-recently-grown first) hunting for a slot; on miss across
//! all current spans, a fresh span is mmap'd via
//! [`Span::new_for_class`] and appended.
//!
//! Free is `Span::free_slot` after a same-class `contains(ptr)`
//! scan to dispatch — O(n_spans_in_class), bounded by
//! `PER_CLASS_CAP`. Phase 2a item 3 will introduce a global
//! `SpanRegistry` for O(1) ptr→span lookup, retiring the scan.
//!
//! Public surface is **invariant** vs the pre-Phase-2a shape:
//! `Allocator::new` / `alloc(size)` / `dealloc(ptr, size)` /
//! `bucket_for(size)` / `mapped_bytes()`. `extern_api.rs` keeps
//! working unchanged.
//!
//! Size class table stays 9 power-of-two buckets for this commit;
//! Phase 2a item 2.5 expands to a Go-style 32-class fine-grained
//! table in a separate atomic commit (decoupled from the span
//! migration to keep bisect surface clean).

use core::ptr::NonNull;

use crate::span::{SPAN_LEN, Span};

/// Power-of-two size classes covered by the per-class span pool.
/// Requests larger than the last entry route to `super::large`.
pub const SIZE_CLASSES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Max active spans per size class. With the mailrs fork's
/// `SPAN_LEN = 512 KB`, 4096 spans × 512 KB = 2 GB per class
/// addressable arena. (Pre-bump: 16 KB span × 4096 = 64 MB per class,
/// which mailrs-server boot exhausted on the 4096-byte class —
/// `alloc_sized` returned null → `handle_alloc_error`. See the
/// SPAN_LEN docstring in `span.rs` for the incident.) With 9
/// classes the upper bound is 18 GB; in practice only a few classes
/// grow, the rest stay at zero spans. Metadata cost
/// (Option<Span> ≈ 32 B) is 4096 × 32 × 9 ≈ 1.15 MB static — fine
/// for a static mut.
pub const PER_CLASS_CAP: usize = 4096;

/// Backwards-compat alias: pre-Phase-2a callers using
/// `MAX_PAGES` semantically meant "total span budget across all
/// classes". The new shape budgets per-class; this constant
/// remains exported for any external auditor scripts that
/// reference it (= `PER_CLASS_CAP × SIZE_CLASSES.len()` upper
/// bound).
pub const MAX_PAGES: usize = PER_CLASS_CAP * SIZE_CLASSES.len();

const NONE_SPAN: Option<Span> = None;
const EMPTY_CLASS_ARRAY: [Option<Span>; PER_CLASS_CAP] = [NONE_SPAN; PER_CLASS_CAP];

pub struct Allocator {
    /// Per-class span pool. Each entry is `Some(Span)` if that
    /// pool slot is populated, `None` if unused. Population grows
    /// from index 0 monotonically per class via `class_cur`.
    classes: [[Option<Span>; PER_CLASS_CAP]; SIZE_CLASSES.len()],
    /// Per-class population cursor — number of spans currently in
    /// `classes[class_idx]`. New spans append at index `class_cur`;
    /// `class_cur` is bounded by `PER_CLASS_CAP`.
    class_cur: [u16; SIZE_CLASSES.len()],
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub const fn new() -> Self {
        Allocator {
            classes: [EMPTY_CLASS_ARRAY; SIZE_CLASSES.len()],
            class_cur: [0; SIZE_CLASSES.len()],
        }
    }

    /// Round `size` up to the next size class index; returns
    /// `None` if `size` exceeds the largest bucket.
    pub fn bucket_for(size: usize) -> Option<usize> {
        if size == 0 {
            return Some(0);
        }
        SIZE_CLASSES.iter().position(|&c| size <= c)
    }

    /// Allocate `size` bytes from the appropriate size-class pool.
    /// Returns `None` on OOM (per-class cap exceeded or kernel
    /// mmap failure). `size` past the largest class returns
    /// `None` — caller routes to `super::large`.
    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        let bucket = Self::bucket_for(size)?;
        let class_size = SIZE_CLASSES[bucket];

        // 1. LIFO span scan — try most-recently-grown spans first.
        //    Recent spans are likely the same span the immediately-
        //    prior alloc came from (TLAB-ish locality even before
        //    Phase 2b TLAB ships).
        let cur = self.class_cur[bucket] as usize;
        for i in (0..cur).rev() {
            if let Some(span) = self.classes[bucket][i].as_mut() {
                if let Some(p) = span.alloc_slot() {
                    return Some(p);
                }
            }
        }

        // 2. All current spans full — grow.
        if cur >= PER_CLASS_CAP {
            return None;
        }
        let mut new_span = Span::new_for_class(class_size, bucket as u8).ok()?;
        let p = new_span.alloc_slot()?;
        self.classes[bucket][cur] = Some(new_span);
        self.class_cur[bucket] += 1;
        Some(p)
    }

    /// Release a previously-allocated block. `size` must be the
    /// SAME value passed to `alloc` (size-class allocator has no
    /// per-block size metadata in this API; caller bookkeeping
    /// required — `super::extern_api` Layer 2 shim handles this
    /// via SHIM_HEADER for libc-compat consumers).
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by `alloc(size)`, and not
    /// already freed (double-free is UB and will corrupt the
    /// owning span's freelist).
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize) {
        let Some(bucket) = Self::bucket_for(size) else {
            // Out of bucket range — caller should have used
            // large_alloc/large_free; silently leak to keep the
            // invariant simple.
            return;
        };
        // Dispatch ptr to its owning span: same-class scan.
        // Phase 2a item 3 replaces this with O(1) registry lookup.
        let cur = self.class_cur[bucket] as usize;
        for i in 0..cur {
            if let Some(span) = self.classes[bucket][i].as_mut() {
                if span.contains(ptr) {
                    // SAFETY: ptr is contained in this span; caller's
                    // outer Safety invariant says ptr was from a
                    // matching `alloc(size)`, which placed it in
                    // exactly this size class.
                    unsafe { span.free_slot(ptr) };
                    return;
                }
            }
        }
        // ptr not in any span — silently drop (matches legacy
        // behavior: mis-sized free is leak, not UB).
    }

    /// Total bytes addressable from the kernel via this allocator.
    /// = `sum over classes of (active_spans × SPAN_LEN)`.
    /// Diagnostic, not a runtime hot-path.
    pub fn mapped_bytes(&self) -> usize {
        let mut sum = 0usize;
        for cur in self.class_cur.iter() {
            sum += (*cur as usize) * SPAN_LEN;
        }
        sum
    }
}

// Suppress unused — the `NonNull` import is needed for the
// public re-export path some downstream tests reach for; without
// it the file fails compile when those tests poke internals.
// (Kept here so a future refactor that moves NonNull users out
// won't silently break re-exports.)
#[allow(dead_code)]
const _PHANTOM_NONNULL_USE: Option<NonNull<u8>> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Build an `Allocator` directly on the heap, bypassing the
    /// ~1 MB stack copy that `Box::new(Allocator::new())` does in
    /// debug builds (where the on-stack temporary trips the
    /// 2 MB default test-thread stack on Linux CI runners).
    /// Safe because `Option<Span>` has the NonNull niche, so the
    /// all-zero bit pattern is a valid `Allocator::new()` value
    /// (all spans = None, all class cursors = 0).
    fn test_alloc() -> Box<Allocator> {
        use core::alloc::Layout;
        let layout = Layout::new::<Allocator>();
        // SAFETY: layout is non-zero; alloc_zeroed returns a
        // pointer to zero-initialised memory of that size+align,
        // and our Allocator type is all-zero-valid.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) } as *mut Allocator;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // SAFETY: ptr was just returned by the global allocator with
        // the matching layout, and the all-zero bit pattern is a
        // valid Allocator.
        unsafe { Box::from_raw(ptr) }
    }

    #[test]
    fn alloc_and_free_recycles() {
        let mut a = test_alloc();
        let p1 = a.alloc(16).expect("alloc 16");
        unsafe { *p1 = 0xab };
        unsafe { a.dealloc(p1, 16) };
        // Next alloc of same bucket should hand back the same
        // block (Span freelist is LIFO).
        let p2 = a.alloc(16).expect("realloc 16");
        assert_eq!(p1, p2, "free list not recycling");
    }

    #[test]
    fn bucket_routing() {
        assert_eq!(Allocator::bucket_for(1), Some(0));
        assert_eq!(Allocator::bucket_for(16), Some(0));
        assert_eq!(Allocator::bucket_for(17), Some(1));
        assert_eq!(Allocator::bucket_for(4096), Some(8));
        assert_eq!(Allocator::bucket_for(4097), None);
    }

    #[test]
    fn cross_span_alloc() {
        let mut a = test_alloc();
        // Fill span 1 with 256-class allocations: 16 KB / 256 = 64
        // slots fits exactly; the 65th should trigger a new span.
        for _ in 0..64 {
            let p = a.alloc(256).expect("alloc 256");
            unsafe { *p = 0xcd };
        }
        let p = a.alloc(256).expect("alloc 256 across spans");
        unsafe { *p = 0xef };
        assert_eq!(a.mapped_bytes(), 2 * SPAN_LEN);
    }

    #[test]
    fn alloc_too_large_returns_none() {
        let mut a = test_alloc();
        assert!(
            a.alloc(8192).is_none(),
            "8192 > max bucket — caller routes to large_alloc"
        );
    }

    #[test]
    fn writable_freshly_mapped() {
        let mut a = test_alloc();
        for size in [16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            let p = a.alloc(size).expect("alloc");
            unsafe {
                for off in 0..size {
                    ptr::write(p.add(off), (off & 0xff) as u8);
                }
                for off in 0..size {
                    assert_eq!(*p.add(off), (off & 0xff) as u8);
                }
            }
        }
    }

    /// Every alloc returns a 16-byte aligned pointer (matches macOS
    /// libc malloc guarantee). SIZE_CLASSES are multiples of 16
    /// and SPAN_LEN is 16K, so the invariant holds by construction —
    /// this test pins it down so future cursor edits in Span can't
    /// silently break alignment for SIMD / `_Atomic` heap reads.
    #[test]
    fn alloc_pointers_are_16_byte_aligned() {
        let mut a = test_alloc();
        for _ in 0..1024 {
            for &size in SIZE_CLASSES.iter() {
                let p = a.alloc(size).expect("alloc") as usize;
                assert_eq!(
                    p & 0xf,
                    0,
                    "alloc({}) returned 0x{:x} (not 16-byte aligned)",
                    size,
                    p
                );
            }
        }
    }

    /// Stress: 100K alloc/free roundtrips across all size classes
    /// without corruption. Catches freelist double-link / cross-
    /// span dispatch bugs that single-shot tests miss.
    #[test]
    fn stress_100k_roundtrips_no_corruption() {
        let mut a = test_alloc();
        let sizes = [16usize, 32, 64, 128, 256, 512, 1024, 2048, 4096];
        for round in 0..100_000usize {
            let size = sizes[round % sizes.len()];
            let p = a.alloc(size).expect("alloc");
            unsafe {
                let header = p as *mut u64;
                let magic = 0xdeadbeef00000000u64 | (round as u64);
                ptr::write(header, magic);
                assert_eq!(ptr::read(header), magic);
                a.dealloc(p, size);
            }
        }
    }

    /// Span-backed shape regression: verify two allocs in the same
    /// class share a single span until that span is full. (Legacy
    /// PageBump shape could mix-size within a page; new Span shape
    /// must NOT.)
    #[test]
    fn same_class_packs_into_one_span() {
        let mut a = test_alloc();
        let p1 = a.alloc(64).expect("alloc 1");
        let p2 = a.alloc(64).expect("alloc 2");
        // Both should be in the same span: addresses differ by
        // slot_size (64 B), not by span_len.
        let delta = (p2 as usize).abs_diff(p1 as usize);
        assert_eq!(delta, 64, "same-class allocs not packed in one span");
        assert_eq!(a.mapped_bytes(), SPAN_LEN, "should be exactly 1 span");
    }
}
