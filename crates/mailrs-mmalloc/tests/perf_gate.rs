//! Per-operation performance budget gate.
//!
//! Asserts `elapsed < budget` for each hot op listed in `BUDGETS.md`.
//! Budgets are set at 5× the measured p95 on a representative dev
//! machine to absorb noise on slower CI runners — see `BUDGETS.md`
//! for the per-op derivation. Numbers are conservative; native
//! linux/amd64 hardware is typically ~30% faster than the
//! rosetta-translated docker run baseline these were measured on.
//!
//! Why integration tests AND criterion benches: criterion warns on
//! regression but doesn't fail CI. Integration tests with
//! `assert!(elapsed < budget)` do — and they run as part of the
//! normal `cargo test --workspace --release` gate.
//!
//! **Release-only**: debug builds run the allocator unoptimised
//! (~7× slower than release on the same hardware); enforcing
//! release-grade budgets on a debug build would false-fail every
//! test. The whole file is `cfg(not(debug_assertions))`-gated so
//! `cargo test` (debug default) skips it entirely; CI runs it via
//! `cargo test --workspace --release`.
//!
//! **Linux-only** — perf on macOS host_stub is `std::alloc::System`
//! so the numbers wouldn't reflect mmalloc.

#![cfg(all(target_os = "linux", not(debug_assertions)))]

use core::alloc::{GlobalAlloc, Layout};
use std::time::{Duration, Instant};

use mailrs_mmalloc::MailrsAllocator;

/// Number of alloc/free pairs per measurement. Larger N averages
/// out noise; we divide elapsed by N to get the per-op time.
const N: usize = 10_000;

/// Per-iter budget for any small-class alloc + free pair (in ns).
/// 5× the p95 ~260 ns measured on linux/amd64 docker; gives
/// headroom for slower CI runners.
const SMALL_BUDGET_NS: u128 = 1_500;

/// Per-iter budget for large-class (mmap path) alloc + free (in ns).
/// 5× the p95 ~2.8 µs for an 8 K alloc.
const LARGE_BUDGET_NS: u128 = 15_000;

fn measure_small(size: usize) -> u128 {
    let a = MailrsAllocator;
    let layout = Layout::from_size_align(size, 8).unwrap();
    // Warm up the TLAB so the first iter doesn't pay the refill cost.
    unsafe {
        let p = a.alloc(layout);
        a.dealloc(p, layout);
    }
    let start = Instant::now();
    for _ in 0..N {
        unsafe {
            let p = a.alloc(layout);
            // Touch the slot so the optimiser can't elide the alloc.
            *p = 0xab;
            a.dealloc(p, layout);
        }
    }
    start.elapsed().as_nanos() / N as u128
}

fn measure_large(size: usize) -> u128 {
    let a = MailrsAllocator;
    let layout = Layout::from_size_align(size, 4096).unwrap();
    // Warm up.
    unsafe {
        let p = a.alloc(layout);
        a.dealloc(p, layout);
    }
    // Large path is expensive (mmap+munmap each iter); use fewer iters
    // so the test doesn't take too long.
    let large_n: usize = 1_000;
    let start = Instant::now();
    for _ in 0..large_n {
        unsafe {
            let p = a.alloc(layout);
            *p = 0xab;
            a.dealloc(p, layout);
        }
    }
    start.elapsed().as_nanos() / large_n as u128
}

#[test]
fn small_alloc_free_under_budget_size_16() {
    let ns = measure_small(16);
    assert!(
        ns < SMALL_BUDGET_NS,
        "alloc(16)+free per-iter {ns} ns >= budget {SMALL_BUDGET_NS} ns"
    );
}

#[test]
fn small_alloc_free_under_budget_size_64() {
    let ns = measure_small(64);
    assert!(
        ns < SMALL_BUDGET_NS,
        "alloc(64)+free per-iter {ns} ns >= budget {SMALL_BUDGET_NS} ns"
    );
}

#[test]
fn small_alloc_free_under_budget_size_256() {
    let ns = measure_small(256);
    assert!(
        ns < SMALL_BUDGET_NS,
        "alloc(256)+free per-iter {ns} ns >= budget {SMALL_BUDGET_NS} ns"
    );
}

#[test]
fn small_alloc_free_under_budget_size_1024() {
    let ns = measure_small(1024);
    assert!(
        ns < SMALL_BUDGET_NS,
        "alloc(1024)+free per-iter {ns} ns >= budget {SMALL_BUDGET_NS} ns"
    );
}

#[test]
fn small_alloc_free_under_budget_size_4096() {
    let ns = measure_small(4096);
    assert!(
        ns < SMALL_BUDGET_NS,
        "alloc(4096)+free per-iter {ns} ns >= budget {SMALL_BUDGET_NS} ns"
    );
}

#[test]
fn large_alloc_free_under_budget_8k() {
    let ns = measure_large(8 * 1024);
    assert!(
        ns < LARGE_BUDGET_NS,
        "alloc(8K)+free per-iter {ns} ns >= budget {LARGE_BUDGET_NS} ns"
    );
}

/// Total wall-clock for the whole perf_gate suite — sanity check that
/// none of the measurements hung beyond a reasonable bound.
#[test]
fn whole_suite_under_30s() {
    let start = Instant::now();
    let _ = measure_small(16);
    let _ = measure_large(8 * 1024);
    assert!(start.elapsed() < Duration::from_secs(30));
}
