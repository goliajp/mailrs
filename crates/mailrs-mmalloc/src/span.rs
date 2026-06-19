//! Span — a contiguous mmap'd region serving one size class.
//!
//! Phase 2a substep 1 of the metal-tier allocator redesign (see
//! `docs/v0.7-A2-finding.md`). Spans are the unit of allocation
//! ownership in the redesigned allocator: each span belongs to a
//! single size class, has N fixed-size slots, and tracks its own
//! free list inline (LIFO via 8-byte next pointers in unused slots).
//!
//! This file is data-structure scaffolding only — no `extern_api`
//! caller yet. Subsequent Phase 2a commits will (a) migrate
//! `size_class::Allocator` to be span-backed, and (b) introduce the
//! Layer 0/1 `core` API that does ptr→span lookup via a registry
//! built on this type.
//!
//! Design references (per design-principles.md 正统 pillar):
//! - Go runtime `src/runtime/mheap.go::mspan` — span-per-size-class
//! - mimalloc `src/page.c` — page-as-span concept
//! - snmalloc slab.h — fixed-size slot iteration

use core::mem::size_of;
use core::ptr::{self, NonNull};

use mailrs_syscall::{Errno, madvise_dontneed, mmap_anon_rw_aligned, munmap};

/// Default span byte length. Bumped from the legacy `PageBump` 16 KB
/// to 512 KB on the mailrs fork: combined with `PER_CLASS_CAP = 4096`,
/// 16 KB capped each size class at 64 MB of addressable arena, which
/// `mailrs-server` boot exhausted on the 4096-byte class (staging
/// reproducer logged `memory allocation of 2520 bytes failed` →
/// `handle_alloc_error`). 512 KB raises the per-class ceiling to
/// 2 GB. Cannot go above 1 MB because `Span::slot_count` is `u16`
/// and `SPAN_LEN / MIN_SLOT(16) ≤ 65535` must hold; 512 KB / 16 =
/// 32 768 leaves a comfortable margin.
pub const SPAN_LEN: usize = 512 * 1024;

/// Minimum slot size — must hold an in-place freelist `next`
/// pointer (8 bytes on 64-bit targets). All size classes are
/// constrained to be ≥ this.
pub const MIN_SLOT: usize = size_of::<*mut u8>();

/// Embedded freelist node — overlays an unused slot. When a slot is
/// free, its first 8 bytes hold the pointer to the next free slot
/// (or null at the end). When a slot is in use, those bytes are
/// the caller's data — the freelist invariant is "the head pointer
/// is the only reader of the next field, and it only reads when
/// the slot is on the free list".
#[repr(C)]
struct FreeNode {
    next: Option<NonNull<FreeNode>>,
}

/// A single span — 16 KB region carved into `slot_count` slots of
/// `slot_size` bytes each, serving one size class.
///
/// Invariants:
/// - `slot_size >= MIN_SLOT`
/// - `slot_size * slot_count <= SPAN_LEN`
/// - `free_count <= slot_count`
/// - Every slot at index `>= bump_high` is owned by `freelist` xor
///   "live" (= handed out and not freed back); slots at index
///   `< bump_high` are not yet handed out (the bump cursor hasn't
///   reached them) and live virgin in the mmap'd region.
pub struct Span {
    /// Base address of the mmap'd region (length = `SPAN_LEN`).
    base: NonNull<u8>,
    /// Size of one slot, in bytes. Constant for the span lifetime.
    slot_size: u16,
    /// Total slots in this span = `SPAN_LEN / slot_size as usize`.
    slot_count: u16,
    /// Slots currently available (free + virgin).
    free_count: u16,
    /// Bump cursor: index of the next never-handed-out slot. When
    /// the freelist is empty and `bump_high < slot_count`, the next
    /// `alloc_slot` peels from the virgin region. After `bump_high`
    /// reaches `slot_count`, allocation is freelist-only.
    bump_high: u16,
    /// Head of the recycled-slot LIFO. `None` = no recycled slots.
    freelist: Option<NonNull<FreeNode>>,
    /// Size class index (0..N). Allows ptr→span→class lookup; the
    /// `core` API uses this to dispatch free without a caller-
    /// supplied size.
    pub class_idx: u8,
    /// `true` while the span has resident pages (alloc_slot has been
    /// called since the last `decommit_pages`). `false` immediately
    /// after `decommit_pages` returns and until the next alloc_slot
    /// touches a page. Used by `Allocator::resident_bytes` to report
    /// RSS-equivalent state.
    pub dirty: bool,
}

impl Span {
    /// Allocate a new span from the kernel, SPAN_LEN-aligned and
    /// sliced for slots of `slot_size` bytes. `slot_size` must be
    /// ≥ `MIN_SLOT` and ≤ `SPAN_LEN`; returns an error on bad input
    /// or mmap failure.
    ///
    /// The SPAN_LEN-alignment is load-bearing for `core`'s O(1)
    /// ptr→span lookup: given any interior `ptr`, the owning span's
    /// base is recoverable as `ptr & !(SPAN_LEN - 1)` with no
    /// registry walk. `mmap_anon_rw_aligned` over-allocates +
    /// trims to guarantee the alignment.
    pub fn new_for_class(slot_size: usize, class_idx: u8) -> Result<Self, Errno> {
        debug_assert!(slot_size >= MIN_SLOT, "slot_size < MIN_SLOT");
        debug_assert!(slot_size <= SPAN_LEN, "slot_size > SPAN_LEN");
        let p = mmap_anon_rw_aligned(SPAN_LEN, SPAN_LEN)?;
        // SAFETY: mmap_anon_rw_aligned returned Ok ⇒ p is non-null,
        // SPAN_LEN-aligned, and points to SPAN_LEN bytes of writable
        // memory.
        let base = unsafe { NonNull::new_unchecked(p) };
        debug_assert!(
            (base.as_ptr() as usize) & (SPAN_LEN - 1) == 0,
            "Span::new_for_class: aligned mmap returned unaligned base"
        );
        let slot_count = (SPAN_LEN / slot_size) as u16;
        Ok(Span {
            base,
            slot_size: slot_size as u16,
            slot_count,
            free_count: slot_count,
            bump_high: 0,
            freelist: None,
            class_idx,
            dirty: false, // freshly mmap'd pages are zero, no commit yet
        })
    }

    /// Pop one slot. Returns `None` if the span is fully used.
    /// LIFO order — recently freed slots are handed out first
    /// (cache-friendly). Marks the span `dirty` (= has resident
    /// pages); the next `decommit_pages` will madvise them again.
    pub fn alloc_slot(&mut self) -> Option<*mut u8> {
        if let Some(node) = self.freelist.take() {
            // SAFETY: freelist head was written by a prior `free_slot`,
            // which only pushes pointers it received from `alloc_slot`
            // (= slots within this span). Reading `next` is sound.
            self.freelist = unsafe { node.as_ref().next };
            self.free_count -= 1;
            self.dirty = true;
            return Some(node.as_ptr() as *mut u8);
        }
        if self.bump_high < self.slot_count {
            let offset = self.bump_high as usize * self.slot_size as usize;
            // SAFETY: offset is bounded by slot_count * slot_size ≤
            // SPAN_LEN; base is a valid SPAN_LEN-byte region.
            let p = unsafe { self.base.as_ptr().add(offset) };
            self.bump_high += 1;
            self.free_count -= 1;
            self.dirty = true;
            return Some(p);
        }
        None
    }

    /// Tell the kernel we don't need this span's pages right now —
    /// `madvise(base, SPAN_LEN, MADV_DONTNEED)`. The VMA stays
    /// mapped (the next `alloc_slot` page-faults fresh zero pages
    /// in), but resident pages are returned to the OS immediately
    /// and RSS for the process drops.
    ///
    /// **Caller MUST ensure the span has no live slots** — calling
    /// this while any slot is handed out to user code would zap
    /// their data. `Allocator::dealloc_hinted` checks `is_empty()`
    /// before calling.
    ///
    /// **Critical state reset**: the embedded freelist (`next`
    /// pointers stored in slot first-8-bytes) lives in the very
    /// pages being decommitted — `madvise` zeros them, so the
    /// existing `freelist` head would walk into zero-filled
    /// territory on the next pop (= "freelist ends here"). Without
    /// a reset, all slots beyond the head would be invisible to
    /// `alloc_slot`. We restore `bump_high = 0`, `freelist = None`
    /// so `alloc_slot` bumps from offset 0 across the now-virgin
    /// region, matching the kernel's view that the pages are fresh.
    /// `free_count` was already `slot_count` (the caller's empty
    /// precondition), so no change there.
    pub fn decommit_pages(&mut self) {
        debug_assert_eq!(
            self.free_count, self.slot_count,
            "decommit_pages called with live slots — UB risk"
        );
        // SAFETY: self.base is the SPAN_LEN-aligned mmap'd region
        // returned by `Span::new_for_class`; passing it back to
        // madvise is well-formed. Errors are swallowed — there's
        // no recovery for a failed madvise (would just leave the
        // pages resident until the next alloc; not a correctness
        // issue).
        let _ = unsafe { madvise_dontneed(self.base.as_ptr(), SPAN_LEN) };
        self.dirty = false;
        // Reset slot-tracking state to the post-mmap "virgin" shape.
        // The kernel has dropped the pages, so any freelist next ptrs
        // we held are now stale (would read as 0 on first re-access).
        self.bump_high = 0;
        self.freelist = None;
    }

    /// Push a slot back onto the freelist.
    ///
    /// # Safety
    ///
    /// `ptr` must be a slot pointer originally returned by
    /// `alloc_slot` on **this** span, and not already freed. The
    /// caller is responsible for span-ownership routing (the metal-
    /// tier `core` API does this via a `SpanRegistry` lookup).
    pub unsafe fn free_slot(&mut self, ptr: *mut u8) {
        debug_assert!(self.contains(ptr), "free_slot: ptr not in span");
        let node = ptr as *mut FreeNode;
        // SAFETY: ptr is a slot in this span (debug_assert above);
        // we overwrite the first 8 bytes with the freelist `next`
        // pointer per the embedded-LIFO invariant.
        unsafe {
            ptr::write(
                node,
                FreeNode {
                    next: self.freelist,
                },
            );
            self.freelist = Some(NonNull::new_unchecked(node));
        }
        self.free_count += 1;
    }

    /// Check whether `ptr` falls within this span's mmap'd region.
    /// O(1). Used by `SpanRegistry` ptr→span lookup and by debug
    /// assertions in `free_slot`.
    #[inline]
    pub fn contains(&self, ptr: *mut u8) -> bool {
        let base = self.base.as_ptr() as usize;
        let p = ptr as usize;
        p >= base && p < base + SPAN_LEN
    }

    /// Base address of the mmap'd region. Diagnostic / registry use.
    #[inline]
    pub fn base(&self) -> *mut u8 {
        self.base.as_ptr()
    }

    /// True iff all slots are on the freelist (or never handed out).
    /// Phase 2c will use this to return empty spans to the central
    /// pool for cross-class recycling.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.free_count == self.slot_count
    }

    /// True iff no slots are available — every slot is live.
    /// Phase 2b TLAB refill will skip full spans.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.free_count == 0
    }

    /// Slot size for this span, in bytes.
    #[inline]
    pub fn slot_size(&self) -> usize {
        self.slot_size as usize
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        // SAFETY: base was mmap'd via mmap_anon_rw with SPAN_LEN
        // bytes; munmap with the same (addr, len) is well-formed.
        // Ignore munmap errors at Drop time — there's no recovery.
        let _ = unsafe { munmap(self.base.as_ptr(), SPAN_LEN) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_for_class_sizes_slot_count_correctly() {
        let s = Span::new_for_class(16, 0).expect("alloc span");
        assert_eq!(s.slot_size(), 16);
        assert_eq!(s.slot_count as usize, SPAN_LEN / 16);
        assert_eq!(s.free_count, s.slot_count);
        assert!(s.is_empty());
        assert!(!s.is_full());
    }

    #[test]
    fn alloc_slot_bumps_then_recycles() {
        let mut s = Span::new_for_class(64, 1).expect("alloc span");
        // First N allocs bump from virgin region — addresses should
        // be monotone-increasing in slot_size strides.
        let p0 = s.alloc_slot().expect("alloc 0");
        let p1 = s.alloc_slot().expect("alloc 1");
        assert_eq!(p1 as usize - p0 as usize, 64);
        // Free p0; next alloc should re-hand it (LIFO).
        unsafe { s.free_slot(p0) };
        let p2 = s.alloc_slot().expect("alloc 2");
        assert_eq!(p0, p2, "freelist not recycling LIFO");
    }

    #[test]
    fn alloc_slot_returns_none_when_full() {
        let mut s = Span::new_for_class(4096, 3).expect("alloc span");
        let slot_count = SPAN_LEN / 4096;
        let mut ptrs = vec![];
        for _ in 0..slot_count {
            ptrs.push(s.alloc_slot().expect("alloc"));
        }
        assert!(s.is_full());
        assert!(
            s.alloc_slot().is_none(),
            "alloc past slot_count should fail"
        );
        // Free one — span should report not-full.
        unsafe { s.free_slot(ptrs[0]) };
        assert!(!s.is_full());
        assert!(s.alloc_slot().is_some(), "alloc after free should succeed");
    }

    #[test]
    fn contains_classifies_addresses() {
        let s = Span::new_for_class(32, 0).expect("alloc span");
        let base = s.base() as usize;
        assert!(s.contains(base as *mut u8));
        assert!(s.contains((base + SPAN_LEN - 1) as *mut u8));
        assert!(!s.contains((base - 1) as *mut u8));
        assert!(!s.contains((base + SPAN_LEN) as *mut u8));
    }

    #[test]
    fn slot_writes_are_isolated() {
        // Stress: alloc all slots, write a per-slot magic, verify no
        // bleed-through. Catches accidental slot overlap / off-by-one.
        let mut s = Span::new_for_class(256, 4).expect("alloc span");
        let mut allocs = vec![];
        for i in 0..s.slot_count {
            let p = s.alloc_slot().expect("alloc");
            // Write i as u64 at slot start; remaining bytes untouched.
            unsafe { ptr::write(p as *mut u64, i as u64) };
            allocs.push((p, i));
        }
        for (p, i) in &allocs {
            let v = unsafe { ptr::read(*p as *const u64) };
            assert_eq!(v, *i as u64, "slot {} overwritten", i);
        }
    }

    #[test]
    fn freelist_roundtrip_stress() {
        let mut s = Span::new_for_class(64, 1).expect("alloc span");
        // Fill the span, free in reverse, alloc again — LIFO order
        // should hand back the most-recently-freed slot first.
        let mut ptrs = vec![];
        for _ in 0..s.slot_count {
            ptrs.push(s.alloc_slot().expect("alloc"));
        }
        for p in ptrs.iter().rev() {
            unsafe { s.free_slot(*p) };
        }
        assert!(s.is_empty());
        // Reverse-of-reverse = original order on re-alloc.
        for expected in ptrs.iter() {
            let p = s.alloc_slot().expect("realloc");
            assert_eq!(p, *expected, "LIFO order violated");
        }
    }

    #[test]
    fn class_idx_is_preserved() {
        let s = Span::new_for_class(128, 42).expect("alloc span");
        assert_eq!(s.class_idx, 42);
    }
}
