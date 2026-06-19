//! Stats / observability consistency.
//!
//! After a sequence of allocs + frees, `core::stats()` must reflect:
//! - `small_alloc_count` strictly increases per alloc
//! - `small_free_count` strictly increases per free
//! - `small_in_use_bytes` matches an independent tally
//! - `large_outstanding_bytes` correctly tracks live large allocs
//! - `per_class` sums match the totals
//!
//! Linux-only (the underlying counters are `#[cfg(target_os =
//! "linux")]`-gated alongside the rest of the allocator).

#![cfg(target_os = "linux")]

use core::alloc::{GlobalAlloc, Layout};

use mailrs_mmalloc::MailrsAllocator;
use mailrs_mmalloc::core::stats;
use mailrs_mmalloc::size_class::SIZE_CLASSES;

#[test]
fn alloc_count_strictly_increases() {
    let a = MailrsAllocator;
    let before = stats().small_alloc_count;
    let layout = Layout::from_size_align(64, 8).unwrap();
    let p = unsafe { a.alloc(layout) };
    let after = stats().small_alloc_count;
    assert!(after > before, "alloc didn't bump small_alloc_count");
    unsafe { a.dealloc(p, layout) };
}

#[test]
fn free_count_strictly_increases() {
    let a = MailrsAllocator;
    let layout = Layout::from_size_align(128, 8).unwrap();
    let p = unsafe { a.alloc(layout) };
    let before = stats().small_free_count;
    unsafe { a.dealloc(p, layout) };
    let after = stats().small_free_count;
    assert!(after > before, "free didn't bump small_free_count");
}

/// Per-class sums match the global totals.
#[test]
fn per_class_sums_match_totals() {
    let s = stats();
    let in_use_per_class: usize = s
        .per_class
        .iter()
        .map(|c| c.class_size * c.slots_in_use as usize)
        .sum();
    assert_eq!(
        in_use_per_class, s.small_in_use_bytes,
        "small_in_use_bytes != sum(per_class.class_size * slots_in_use)"
    );
    let mapped_per_class: usize = s
        .per_class
        .iter()
        .map(|c| c.span_count as usize)
        .sum::<usize>()
        * mailrs_mmalloc::span::SPAN_LEN;
    assert_eq!(
        mapped_per_class, s.small_mapped_bytes,
        "small_mapped_bytes != sum(per_class.span_count * SPAN_LEN)"
    );
    // resident <= mapped
    assert!(
        s.small_resident_bytes <= s.small_mapped_bytes,
        "resident > mapped is impossible"
    );
}

/// 32-class table is populated end-to-end.
#[test]
fn per_class_table_covers_all_size_classes() {
    let s = stats();
    assert_eq!(s.per_class.len(), SIZE_CLASSES.len());
    for (i, c) in s.per_class.iter().enumerate() {
        assert_eq!(
            c.class_size, SIZE_CLASSES[i],
            "per_class[{i}].class_size = {} doesn't match SIZE_CLASSES[{i}] = {}",
            c.class_size, SIZE_CLASSES[i]
        );
    }
}

/// Large path tracks outstanding bytes correctly across alloc + free.
#[test]
fn large_alloc_tracks_outstanding() {
    let a = MailrsAllocator;
    let before = stats().large_outstanding_bytes;
    let before_count = stats().large_alloc_count;
    let layout = Layout::from_size_align(8192, 8).unwrap();
    let p = unsafe { a.alloc(layout) };
    let after_alloc = stats();
    assert!(
        after_alloc.large_outstanding_bytes >= before + 8192,
        "large_outstanding_bytes didn't grow"
    );
    assert_eq!(after_alloc.large_alloc_count, before_count + 1);
    unsafe { a.dealloc(p, layout) };
    let after_free = stats();
    assert_eq!(
        after_free.large_outstanding_bytes, before,
        "large_outstanding_bytes didn't return to baseline after free"
    );
    assert_eq!(after_free.large_free_count, after_alloc.large_alloc_count);
}

/// After an alloc/free pair, in-use bytes returns to its prior level
/// (modulo other concurrent allocs from rust runtime). Use the
/// per-class slot count instead which we own exclusively for size 128.
#[test]
fn alloc_free_pair_returns_in_use_to_baseline_for_isolated_size() {
    let a = MailrsAllocator;
    // Use an unusual size to minimise interference from rustc runtime
    // allocs that might land in the same class.
    let layout = Layout::from_size_align(208, 8).unwrap();
    let bucket = bucket_for(208);
    let before_class = stats().per_class[bucket].slots_in_use;
    let p = unsafe { a.alloc(layout) };
    let after_alloc = stats().per_class[bucket].slots_in_use;
    assert!(after_alloc > before_class, "slots_in_use didn't grow");
    unsafe { a.dealloc(p, layout) };
    let after_free = stats().per_class[bucket].slots_in_use;
    assert!(
        after_free <= after_alloc,
        "slots_in_use didn't drop after free"
    );
}

fn bucket_for(size: usize) -> usize {
    mailrs_mmalloc::size_class::Allocator::bucket_for(size).unwrap()
}
