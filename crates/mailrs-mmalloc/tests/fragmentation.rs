//! Internal-fragmentation gate.
//!
//! M5 expanded the size-class table from 9 power-of-two entries
//! (worst-case 50% waste on the boundary, e.g. 17 → 32) to 32
//! Go-style entries (worst-case ~29% on the worst boundary, but
//! average < 12.5% across a uniform-random workload).
//!
//! This test measures the AVG internal waste (slot_size −
//! request_size) / slot_size on a uniform-random workload across
//! [16, 4096] and asserts it stays below 12.5%. Worst-case per
//! request can exceed that; the average is what real workloads pay.

#![cfg(target_os = "linux")]

use mailrs_mmalloc::size_class::{Allocator, SIZE_CLASSES};

const SAMPLES: usize = 100_000;
const SEED: u64 = 0xabcd_ef01_2345_6789;
/// Average internal waste budget across uniform-random sizes.
/// Below this is "Go-class fragmentation behaviour".
const AVG_WASTE_BUDGET: f64 = 0.125;

fn xs(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn avg_internal_waste_under_budget() {
    let mut rng = SEED;
    let mut total_waste = 0u64;
    let mut total_slot = 0u64;
    let max_class = SIZE_CLASSES[SIZE_CLASSES.len() - 1];
    for _ in 0..SAMPLES {
        let r = xs(&mut rng);
        // Uniform request in [1, max_class]
        let request = ((r as usize) % max_class) + 1;
        let bucket = Allocator::bucket_for(request).expect("in-range");
        let slot = SIZE_CLASSES[bucket];
        total_waste += (slot - request) as u64;
        total_slot += slot as u64;
    }
    let avg_waste_ratio = total_waste as f64 / total_slot as f64;
    println!(
        "avg waste over {SAMPLES} uniform requests in [1, {max_class}]: \
         {:.4} ({}/{})",
        avg_waste_ratio, total_waste, total_slot
    );
    assert!(
        avg_waste_ratio < AVG_WASTE_BUDGET,
        "avg internal waste {avg_waste_ratio:.4} exceeds budget {AVG_WASTE_BUDGET}"
    );
}

/// Sanity: every request in [1, max_class] routes to a class that
/// is at least as large as the request — no smaller-than-asked-for
/// slot ever gets handed out.
#[test]
fn no_under_sized_slot_ever() {
    let max_class = SIZE_CLASSES[SIZE_CLASSES.len() - 1];
    for request in 1..=max_class {
        let bucket = Allocator::bucket_for(request).expect("in-range");
        let slot = SIZE_CLASSES[bucket];
        assert!(slot >= request, "size {request} → slot {slot} < request");
    }
}
