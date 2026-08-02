//! `AllocatorStats` — the diagnostic snapshot. Walks every class, so
//! not for a hot path.

use core::sync::atomic::Ordering;

use crate::size_class::{PerClassStats, SIZE_CLASSES};
use crate::thread_cache;

use super::*;

/// Aggregate allocator stats — snapshot of internal state for
/// diagnostics. NOT cheap (walks `Allocator::classes` to build
/// `per_class`), so don't call from a hot path; intended for an
/// admin endpoint or a periodic dump.
#[derive(Clone, Copy, Debug)]
pub struct AllocatorStats {
    /// Total VMA bytes from small spans (includes decommitted spans
    /// — their VMA is still mapped, just not resident).
    pub small_mapped_bytes: usize,
    /// VMA bytes from small spans currently backed by resident pages
    /// (`Span::dirty == true`).
    pub small_resident_bytes: usize,
    /// Bytes currently in user code's hands across all size classes
    /// (= sum over spans of `(slot_count - free_count) * slot_size`).
    /// `mapped - in_use` is the "free but reserved" overhead.
    pub small_in_use_bytes: usize,
    /// Lifetime cumulative count of successful small allocs.
    pub small_alloc_count: u64,
    /// Lifetime cumulative count of successful small frees.
    pub small_free_count: u64,
    /// Currently-live (page-rounded) bytes from the large-path
    /// (`size > SIZE_CLASSES.last()`) allocs.
    pub large_outstanding_bytes: usize,
    /// Lifetime cumulative count of large allocs.
    pub large_alloc_count: u64,
    /// Lifetime cumulative count of large frees.
    pub large_free_count: u64,
    /// Per-class breakdown for the 32 small-allocator classes.
    pub per_class: [PerClassStats; SIZE_CLASSES.len()],
    /// Number of per-thread cache slots currently claimed by live
    /// threads (out of `thread_cache::THREAD_SLOTS = 64`).
    pub claimed_thread_cache_slots: usize,
}

/// Snapshot the allocator's current state. Walks the per-class
/// arrays — O(SIZE_CLASSES × class_cur). Cheap enough to call from
/// a `/api/alloc-stats` endpoint at human cadence; do NOT call per
/// alloc/free.
pub fn stats() -> AllocatorStats {
    lock();
    let per_class = unsafe { (*&raw const CORE_ALLOC).stats() };
    let small_mapped_bytes = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    let small_resident_bytes = unsafe { (*&raw const CORE_ALLOC).resident_bytes() };
    unlock();
    let small_in_use_bytes = per_class
        .iter()
        .map(|c| c.class_size * c.slots_in_use as usize)
        .sum();
    AllocatorStats {
        small_mapped_bytes,
        small_resident_bytes,
        small_in_use_bytes,
        small_alloc_count: SMALL_ALLOC_COUNT.load(Ordering::Relaxed),
        small_free_count: SMALL_FREE_COUNT.load(Ordering::Relaxed),
        large_outstanding_bytes: LARGE_OUTSTANDING_BYTES.load(Ordering::Relaxed),
        large_alloc_count: LARGE_ALLOC_COUNT.load(Ordering::Relaxed),
        large_free_count: LARGE_FREE_COUNT.load(Ordering::Relaxed),
        per_class,
        claimed_thread_cache_slots: thread_cache::claimed_count(),
    }
}

// ============================================================
// Layer 0 — alloc / free (size recovered from registry)
// ============================================================
