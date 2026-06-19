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

/// Outcome of `Allocator::alloc_with_idx` — carries enough info for
/// an external reverse-index (`super::core::SpanRegistry`) to register
/// the (base, class_idx, idx) tuple needed for O(1) ptr→span dispatch
/// on subsequent `free`.
pub struct AllocOutcome {
    /// The allocated slot pointer (caller-visible).
    pub ptr: *mut u8,
    /// Index of the owning span in `classes[class_idx]`. Stable for
    /// the lifetime of the span (tombstone-based dealloc keeps it
    /// constant).
    pub idx_in_class: u16,
    /// `true` iff this alloc grew a new span (the caller must register
    /// the span's base in the external registry). For a hit on an
    /// existing span: `false`.
    pub grew_span: bool,
}

pub struct Allocator {
    /// Per-class span pool. Each entry is `Some(Span)` if that pool
    /// slot is populated, `None` if it's either never been populated
    /// OR has been freed (tombstone). Tombstones keep the index stable
    /// for the external `SpanRegistry` — `dealloc_hinted` leaves a
    /// `None` in place instead of compacting the array.
    classes: [[Option<Span>; PER_CLASS_CAP]; SIZE_CLASSES.len()],
    /// Per-class high-water mark — the highest index ever populated
    /// (live OR tombstone). New spans first try to fill the lowest
    /// tombstone in `[0..class_cur)`; if none, append at `class_cur`
    /// and bump it. Never decremented (would invalidate registry idx).
    class_cur: [u16; SIZE_CLASSES.len()],
    /// Per-class count of currently live (`Some`) entries. Used by
    /// `mapped_bytes()` for an O(1) tally instead of scanning the
    /// whole class array on every call.
    class_live: [u16; SIZE_CLASSES.len()],
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
            class_live: [0; SIZE_CLASSES.len()],
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
    /// Returns `None` on OOM. Backwards-compatible wrapper around
    /// `alloc_with_idx` for callers that don't need the span-index
    /// info (= tests + Layer 0 paths).
    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        self.alloc_with_idx(size).map(|o| o.ptr)
    }

    /// Allocate + report the owning span's index. Used by `super::core`
    /// to register `(span_base, class_idx, idx_in_class)` in the
    /// `SpanRegistry` so subsequent O(1) `dealloc_hinted` works.
    ///
    /// `grew_span` is `true` iff this alloc grew a new span (either
    /// appending past `class_cur` or filling a tombstone slot). In
    /// both cases the caller registers the (just-grown) span.
    pub fn alloc_with_idx(&mut self, size: usize) -> Option<AllocOutcome> {
        let bucket = Self::bucket_for(size)?;
        let class_size = SIZE_CLASSES[bucket];
        let cur = self.class_cur[bucket] as usize;

        // 1. LIFO span scan — try most-recently-grown live spans first.
        //    Skips tombstones (Nones) along the way; bounded by cur.
        for i in (0..cur).rev() {
            if let Some(span) = self.classes[bucket][i].as_mut()
                && let Some(p) = span.alloc_slot()
            {
                return Some(AllocOutcome {
                    ptr: p,
                    idx_in_class: i as u16,
                    grew_span: false,
                });
            }
        }

        // 2. All current spans full or tombstoned — need a fresh span.
        //    First look for a tombstone hole in [0..cur) to reuse the
        //    lower idx (keeps cur tight); else append at cur.
        let mut insert_at = cur;
        for i in 0..cur {
            if self.classes[bucket][i].is_none() {
                insert_at = i;
                break;
            }
        }
        if insert_at == cur && cur >= PER_CLASS_CAP {
            return None;
        }
        let mut new_span = Span::new_for_class(class_size, bucket as u8).ok()?;
        let p = new_span.alloc_slot()?;
        self.classes[bucket][insert_at] = Some(new_span);
        if insert_at == cur {
            self.class_cur[bucket] += 1;
        }
        self.class_live[bucket] += 1;
        Some(AllocOutcome {
            ptr: p,
            idx_in_class: insert_at as u16,
            grew_span: true,
        })
    }

    /// O(1) dealloc using the registry-provided `(class_idx,
    /// idx_in_class)` hint. Jumps straight to the owning `Span` without
    /// scanning. If the free empties the span, the span's pages are
    /// `madvise(MADV_DONTNEED)`'d — VMA stays mapped, RSS drops,
    /// next `alloc_slot` on the same span gets fresh zero pages via
    /// page fault. No drop, no compaction, no SpanRegistry change;
    /// `class_live` stays unchanged.
    ///
    /// Returns `true` iff this free decommitted the span (caller can
    /// use this for stats / tracing); `false` if the span survived
    /// with live slots OR the hint was stale.
    ///
    /// # Safety
    ///
    /// `ptr` must be a slot pointer that was returned by an `alloc`
    /// call on the span currently at `classes[class_idx][idx_in_class]`,
    /// not already freed.
    pub unsafe fn dealloc_hinted(
        &mut self,
        ptr: *mut u8,
        class_idx: usize,
        idx_in_class: u16,
    ) -> bool {
        if class_idx >= SIZE_CLASSES.len() {
            return false;
        }
        let idx = idx_in_class as usize;
        let Some(cell) = self.classes[class_idx].get_mut(idx) else {
            return false;
        };
        let Some(span) = cell.as_mut() else {
            return false;
        };
        debug_assert!(
            span.contains(ptr),
            "dealloc_hinted: hint ({class_idx},{idx}) doesn't own ptr"
        );
        // SAFETY: caller's outer Safety invariant — ptr came from
        // a matching alloc on this exact span.
        unsafe { span.free_slot(ptr) };
        if span.is_empty() && span.dirty {
            span.decommit_pages();
            return true;
        }
        false
    }

    /// Legacy O(n_spans_in_class) dealloc — scans the per-class array
    /// for the owning span. Used by callers that don't have a registry
    /// hint (Layer 0 free routes that hit a registry miss; tests that
    /// don't use the registry). Returns `true` if the dealloc
    /// decommitted the span.
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by `alloc(size)`, and not
    /// already freed.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize) -> bool {
        let Some(bucket) = Self::bucket_for(size) else {
            return false;
        };
        let cur = self.class_cur[bucket] as usize;
        for i in 0..cur {
            let owns = matches!(
                self.classes[bucket][i].as_ref(),
                Some(s) if s.contains(ptr)
            );
            if !owns {
                continue;
            }
            // SAFETY: caller's outer invariant + we just verified
            // containment, so this hint is sound.
            return unsafe { self.dealloc_hinted(ptr, bucket, i as u16) };
        }
        false
    }

    /// Total VMA bytes mapped from the kernel via this allocator.
    /// = `sum over classes of (live_spans × SPAN_LEN)`. Includes
    /// decommitted spans (their VMA is still mapped, just not
    /// resident). O(SIZE_CLASSES) — diagnostic, not a hot path.
    pub fn mapped_bytes(&self) -> usize {
        self.class_live
            .iter()
            .map(|live| (*live as usize) * SPAN_LEN)
            .sum()
    }

    /// Total RSS-equivalent bytes — only counts spans currently
    /// `dirty` (= touched since last `decommit_pages`). Decommitted
    /// spans contribute zero. Walks `classes[..]` so it's
    /// O(SIZE_CLASSES × class_cur) — diagnostic only, not a hot path.
    pub fn resident_bytes(&self) -> usize {
        let mut sum = 0usize;
        for (bucket, cur) in self.class_cur.iter().enumerate() {
            for cell in &self.classes[bucket][..*cur as usize] {
                if let Some(span) = cell.as_ref()
                    && span.dirty
                {
                    sum += SPAN_LEN;
                }
            }
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
        // Keep a second slot live so the free below doesn't empty
        // the span (which would `madvise(MADV_DONTNEED)` it and
        // reset bump_high / freelist — the next alloc would then
        // hand a fresh slot at offset 0 instead of recycling p1).
        let p1 = a.alloc(16).expect("alloc 16 #1");
        let _keep = a.alloc(16).expect("alloc 16 #2");
        unsafe { *p1 = 0xab };
        let decommitted = unsafe { a.dealloc(p1, 16) };
        assert!(
            !decommitted,
            "span still has a live slot — should not decommit"
        );
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
        // Fill exactly one span of 256-class allocations; the next
        // alloc must trigger a fresh span and double `mapped_bytes`.
        let slots_per_span = SPAN_LEN / 256;
        for _ in 0..slots_per_span {
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

    /// Empty-span decommit: when the last live slot in a span is
    /// freed, the Allocator madvise's the span's pages
    /// (`MADV_DONTNEED`) — VMA stays mapped (`mapped_bytes()`
    /// unchanged), but RSS drops (`resident_bytes()` falls to 0).
    /// The span object lives on in `classes[][]` ready for instant
    /// reuse without another mmap.
    #[test]
    fn decommit_on_empty_span() {
        let mut a = test_alloc();
        let p = a.alloc(64).expect("alloc");
        assert_eq!(a.mapped_bytes(), SPAN_LEN, "alloc grows one span");
        assert_eq!(a.resident_bytes(), SPAN_LEN, "span is dirty post-alloc");
        let decommitted = unsafe { a.dealloc(p, 64) };
        assert!(decommitted, "last free should decommit");
        assert_eq!(a.mapped_bytes(), SPAN_LEN, "VMA stays mapped");
        assert_eq!(a.resident_bytes(), 0, "decommit drops resident pages");
        // Next alloc reuses the SAME span — page faults re-commit
        // pages on first write, no new mmap.
        let p2 = a.alloc(64).expect("realloc after decommit");
        unsafe { *p2 = 0x77 };
        assert_eq!(a.mapped_bytes(), SPAN_LEN, "still one span (no new mmap)");
        assert_eq!(a.resident_bytes(), SPAN_LEN, "span dirty again post-alloc");
    }

    /// Cross-span decommit: with two spans live, freeing every
    /// slot in the FIRST span decommits only that span and leaves
    /// the second one mapped + dirty + intact. No drop, no
    /// compaction, no array index churn (idx stays stable for
    /// SpanRegistry).
    #[test]
    fn decommit_keeps_classes_intact() {
        let mut a = test_alloc();
        let slots_per_span = SPAN_LEN / 256;
        // Fill span 0 with 256-class allocations.
        let mut span0: Vec<*mut u8> = (0..slots_per_span)
            .map(|_| a.alloc(256).expect("alloc span0"))
            .collect();
        // One more alloc to grow span 1 — keep that ptr live.
        let span1_ptr = a.alloc(256).expect("alloc span1");
        assert_eq!(a.mapped_bytes(), 2 * SPAN_LEN, "two spans mapped");
        assert_eq!(a.resident_bytes(), 2 * SPAN_LEN, "both dirty");
        // Free every slot in span 0 — the last free should decommit it.
        let last = span0.pop().expect("non-empty");
        for p in &span0 {
            assert!(
                !unsafe { a.dealloc(*p, 256) },
                "intermediate free shouldn't decommit"
            );
        }
        assert!(
            unsafe { a.dealloc(last, 256) },
            "last free of span 0 should decommit"
        );
        // VMA still has both spans; RSS only span 1.
        assert_eq!(a.mapped_bytes(), 2 * SPAN_LEN, "both spans still mapped");
        assert_eq!(a.resident_bytes(), SPAN_LEN, "only span 1 dirty");
        // span 1 should still hold its live slot — writeable test.
        unsafe { *span1_ptr = 0xee };
        assert_eq!(unsafe { *span1_ptr }, 0xee, "span 1 must still be live");
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
