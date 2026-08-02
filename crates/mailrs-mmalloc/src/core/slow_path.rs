//! The cold path: everything that runs while `CORE_LOCK` is held,
//! plus TLAB refill and flush.

use core::sync::atomic::Ordering;

use crate::size_class::Allocator;
use crate::span::SPAN_LEN;
use crate::tlab::{TLAB_CACHE_DEPTH, TlabCache};

use super::*;

#[inline]
pub(crate) fn lock() {
    while CORE_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
pub(crate) fn unlock() {
    CORE_LOCK.store(false, Ordering::Release);
}

/// Sentinel returned on zero-size alloc to keep callers from
/// confusing with NULL=OOM. Matches glibc behavior.
#[inline]
pub(crate) fn zero_sentinel() -> *mut u8 {
    &raw const CORE_LOCK as *mut u8
}

// ============================================================
// Layer 1 — alloc_sized / free_sized (hot path, no lookup)
// ============================================================

/// Refill batch size — number of slots `refill_tlab` pulls from the
/// central allocator under one `CORE_LOCK` acquisition. Amortizes lock
/// cost across `BATCH` subsequent TLAB hits. 8 chosen empirically as a
/// compromise: large enough to give meaningful amortization, small
/// enough that a cold class doesn't pre-grow more spans than the
/// caller really needs.
const REFILL_BATCH: usize = 8;

/// Allocate one slot from the central allocator + register the span in
/// `CORE_REGISTRY` (with idx_in_class hint) if a new span was grown.
/// Caller MUST hold `CORE_LOCK`. Returns `null_mut()` on OOM.
///
/// With M2's aligned-mmap + hash-registry, the registered `span_base`
/// is the just-handed-out `p` (first slot in a fresh span is at offset
/// 0, AND `Span::new_for_class` now uses `mmap_anon_rw_aligned` so
/// `p == span_base` and `p & (SPAN_LEN - 1) == 0`). The
/// `idx_in_class` from `AllocOutcome` lets subsequent free hits skip
/// `Allocator::dealloc`'s linear scan via `dealloc_hinted`.
#[inline]
pub(crate) unsafe fn alloc_one_under_lock(class_idx: usize, size: usize) -> *mut u8 {
    let outcome = match unsafe { (*&raw mut CORE_ALLOC).alloc_with_idx(size) } {
        Some(o) => o,
        None => return core::ptr::null_mut(),
    };
    if outcome.grew_span {
        let base = outcome.ptr as usize;
        debug_assert!(
            base & (SPAN_LEN - 1) == 0,
            "alloc_one_under_lock: grown span base {base:#x} not SPAN_LEN-aligned"
        );
        unsafe {
            (*&raw mut CORE_REGISTRY).insert_with_idx(
                base,
                class_idx as u8,
                outcome.idx_in_class,
                SPAN_LEN,
            );
        }
    }
    outcome.ptr
}

/// Free one slot back to the central allocator using the registry's
/// O(1) `(class_idx, idx_in_class)` hint to skip
/// `Allocator::dealloc`'s linear scan. Caller MUST hold `CORE_LOCK`.
///
/// On M4: if the free empties the owning span, `dealloc_hinted`
/// `madvise(MADV_DONTNEED)`'s the span pages — VMA stays mapped, RSS
/// drops, registry entry stays valid (the span object lives on in
/// `Allocator::classes[][]`, ready for instant reuse without re-mmap).
/// So no registry remove on shrink; the entry is permanent for the
/// span's process lifetime.
#[inline]
pub(crate) unsafe fn free_one_under_lock(ptr: *mut u8, size: usize) {
    let class_idx = match Allocator::bucket_for(size) {
        Some(c) => c,
        None => return,
    };
    let idx = match unsafe { (*&raw const CORE_REGISTRY).lookup_full(ptr as usize) } {
        Some((found_class, idx, _)) if found_class as usize == class_idx => idx,
        _ => {
            // Registry miss or class mismatch — fall back to the
            // legacy scan in case the alloc somehow bypassed the
            // registry. Shouldn't fire on Layer 1 allocs that always
            // go through `alloc_one_under_lock`.
            let _decommitted = unsafe { (*&raw mut CORE_ALLOC).dealloc(ptr, size) };
            return;
        }
    };
    let _decommitted = unsafe { (*&raw mut CORE_ALLOC).dealloc_hinted(ptr, class_idx, idx) };
}

/// Refill an empty TLAB class. Strategy:
///
/// 1. **Try `CORE_CENTRAL.pop` up to `REFILL_BATCH` times — no lock.**
///    Central is the lock-free cross-thread slot buffer. If another
///    thread recently overflowed slots of this class, they're sitting
///    here waiting. First `pop` returns the caller's slot; subsequent
///    pops fill the TLAB.
/// 2. **If Central is empty, fall back to `CORE_LOCK + Allocator`.**
///    Pull up to `REFILL_BATCH - already_filled` slots from the
///    central allocator under one lock acquisition.
///
/// # Safety
///
/// `tlab` must point to a `TlabCache` whose owning thread is the
/// caller (= the per-thread-cache slot's `owner_tid == gettid()`).
#[inline(never)]
pub(crate) unsafe fn refill_tlab(tlab: *mut TlabCache, class_idx: usize, size: usize) -> *mut u8 {
    let mut result = core::ptr::null_mut::<u8>();
    let mut filled = 0usize;
    // Phase 1 — drain Central into TLAB. Lock-free, may amortize the
    // entire refill against zero lock acquisitions in steady state.
    while filled < REFILL_BATCH {
        let Some(p) = CORE_CENTRAL.pop(class_idx) else {
            break;
        };
        if filled == 0 {
            result = p;
        } else if !unsafe { (*tlab).push(class_idx, p) } {
            // TLAB full — give back to Central so the slot isn't lost.
            unsafe { CORE_CENTRAL.push(class_idx, p) };
            break;
        }
        filled += 1;
    }
    if filled == REFILL_BATCH {
        return result;
    }
    // Phase 2 — Central exhausted, dip into the locked Allocator for
    // the remainder.
    lock();
    while filled < REFILL_BATCH {
        let p = unsafe { alloc_one_under_lock(class_idx, size) };
        if p.is_null() {
            break;
        }
        if filled == 0 {
            result = p;
        } else if !unsafe { (*tlab).push(class_idx, p) } {
            unsafe { free_one_under_lock(p, size) };
            break;
        }
        filled += 1;
    }
    unlock();
    result
}

/// Flush half the TLAB class **to `CORE_CENTRAL`** (lock-free push)
/// plus the caller's incoming slot. Used when a free arrives and the
/// TLAB class is already at `TLAB_CACHE_DEPTH`. By draining to
/// Central rather than the locked Allocator, this path becomes fully
/// lock-free — perfect for cross-thread free routing (the slots will
/// be popped by whichever thread next misses its TLAB on this class).
///
/// # Safety
///
/// `tlab` must point to a TLAB owned by the caller. `ptr` must be a
/// valid slot pointer of `size` bytes, not already freed.
#[inline(never)]
pub(crate) unsafe fn flush_tlab_and_push(
    tlab: *mut TlabCache,
    class_idx: usize,
    ptr: *mut u8,
    size: usize,
) {
    let _ = size; // Central doesn't need size (slots are class-typed)
    // Drain half the TLAB to Central. Leaves room for the incoming
    // push plus headroom for the next free.
    let target = TLAB_CACHE_DEPTH / 2;
    for _ in 0..target {
        let Some(p) = (unsafe { (*tlab).pop(class_idx) }) else {
            break;
        };
        unsafe { CORE_CENTRAL.push(class_idx, p) };
    }
    // Push the caller's slot. The TLAB now has room (we just drained
    // half) so this should always succeed; if it doesn't (concurrent
    // drain raced us — can't happen for a single-thread-owned TLAB,
    // but guard anyway), fall back to Central.
    if !unsafe { (*tlab).push(class_idx, ptr) } {
        unsafe { CORE_CENTRAL.push(class_idx, ptr) };
    }
}
