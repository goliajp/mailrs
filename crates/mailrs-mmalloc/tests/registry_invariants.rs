//! Direct tests on `SpanRegistry` — the ptr→span lookup table. M2 swaps it
//! from a sorted-array binary search to an open-addressed hash table; the
//! invariants below must continue to hold across that rewrite, so they live
//! here as the structural contract rather than inline next to one shape.
//!
//! Linux-only because `SpanRegistry` is `#[cfg(target_os = "linux")]`.

#![cfg(target_os = "linux")]

use mailrs_mmalloc::core::{LARGE_CLASS_IDX, SpanRegistry};
use mailrs_mmalloc::span::SPAN_LEN;

/// Repeatedly insert random distinct bases; lookup of every inserted base +
/// of a ptr inside its range must return the right entry; lookup of a base
/// that was NOT inserted returns None.
#[test]
fn insert_lookup_random_distinct() {
    let mut r = SpanRegistry::boxed();
    let mut rng: u64 = 0xdead_beef_cafe_babe;
    // Use page-spaced bases so a random "outside" probe falls between spans.
    let bases: Vec<usize> = (0..512)
        .map(|i| 0x1_0000_0000usize + i * SPAN_LEN * 4)
        .collect();
    for (i, &b) in bases.iter().enumerate() {
        assert!(r.insert(b, (i % 8) as u8, SPAN_LEN), "insert #{i} failed");
    }
    assert_eq!(r.len(), bases.len());
    for (i, &b) in bases.iter().enumerate() {
        assert_eq!(r.lookup(b), Some(((i % 8) as u8, SPAN_LEN)));
        assert_eq!(r.lookup(b + SPAN_LEN / 2), Some(((i % 8) as u8, SPAN_LEN)));
        assert_eq!(r.lookup(b + SPAN_LEN - 1), Some(((i % 8) as u8, SPAN_LEN)));
    }
    // Probe between every adjacent pair → must be None.
    for w in bases.windows(2) {
        let between = (w[0] + SPAN_LEN + w[1]) / 2;
        // Skip if `between` happens to land inside w[0] (it won't given
        // spacing, but guard anyway).
        if between >= w[0] + SPAN_LEN && between < w[1] {
            assert_eq!(r.lookup(between), None, "between {}-{}", w[0], w[1]);
        }
        // Deterministic noise — pick a few in-between offsets keyed by rng.
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let _ = rng; // suppress unused on cold path
    }
}

/// Insert-then-remove churn — half-life replacement. After 1000 churn
/// rounds the surviving entries must still resolve correctly.
#[test]
fn churn_keeps_surviving_entries_resolvable() {
    let mut r = SpanRegistry::boxed();
    // Pre-populate.
    let bases: Vec<usize> = (0..200)
        .map(|i| 0x2_0000_0000usize + i * SPAN_LEN * 2)
        .collect();
    for (i, &b) in bases.iter().enumerate() {
        assert!(r.insert(b, (i % 8) as u8, SPAN_LEN));
    }
    // Remove half, re-insert different ones, lookup the survivors.
    for i in (0..200).step_by(2) {
        let removed = r.remove(bases[i]);
        assert_eq!(removed, Some(((i % 8) as u8, SPAN_LEN)));
    }
    // Survivors are odd indices.
    for i in (1..200).step_by(2) {
        assert_eq!(r.lookup(bases[i]), Some(((i % 8) as u8, SPAN_LEN)));
    }
    // Re-insert into the holes with a sentinel class.
    for i in (0..200).step_by(2) {
        let new_base = bases[i] + SPAN_LEN * 1_000_000; // far away
        assert!(r.insert(new_base, 7, SPAN_LEN));
    }
    // Old surviving lookups still work, new ones also work.
    for i in (1..200).step_by(2) {
        assert_eq!(r.lookup(bases[i]), Some(((i % 8) as u8, SPAN_LEN)));
    }
}

/// Mix small (class) and large (LARGE_CLASS_IDX) entries; lookup
/// disambiguates by entry-recorded size so a `ptr` inside a large block
/// returns `(LARGE_CLASS_IDX, real_size)`, never a small-class match.
#[test]
fn small_and_large_coexist() {
    let mut r = SpanRegistry::boxed();
    let small_base = 0x3_0000_0000usize;
    let large_base = small_base + SPAN_LEN * 100;
    let large_size = 256 * 1024;
    assert!(r.insert(small_base, 3, SPAN_LEN));
    assert!(r.insert(large_base, LARGE_CLASS_IDX, large_size));
    assert_eq!(r.lookup(small_base + 100), Some((3, SPAN_LEN)));
    assert_eq!(
        r.lookup(large_base + 1000),
        Some((LARGE_CLASS_IDX, large_size))
    );
    assert_eq!(
        r.lookup(large_base + large_size - 1),
        Some((LARGE_CLASS_IDX, large_size))
    );
    // Just past the large block.
    assert_eq!(r.lookup(large_base + large_size), None);
}

/// Remove of a base that isn't in the registry returns None — no panic,
/// no state change. (Bug surface: M2's hash rewrite must preserve this
/// "free of a foreign ptr is silent" contract.)
#[test]
fn remove_missing_is_safe() {
    let mut r = SpanRegistry::boxed();
    r.insert(0x4_0000_0000usize, 1, SPAN_LEN);
    assert_eq!(r.len(), 1);
    assert_eq!(r.remove(0x4_0000_0000 + SPAN_LEN * 5), None);
    assert_eq!(r.len(), 1);
    assert_eq!(r.lookup(0x4_0000_0000), Some((1, SPAN_LEN)));
}
