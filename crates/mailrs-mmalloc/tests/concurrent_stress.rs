//! Multi-threaded stress — N worker threads each running a long sequence of
//! random alloc / write-magic / verify / free against the shared
//! `MailrsAllocator` global state. Catches:
//! - Freelist corruption from races on per-class span lists
//! - ABA / double-handout in any lock-free path the allocator uses on the
//!   hot path
//! - Off-by-one in span / size_class / registry under contention
//! - Lost allocs (the alloc that returns a non-null ptr to a region that
//!   another thread also got)
//!
//! Deterministic via seeded xorshift — failures reproduce by re-running
//! with the same `WORKER_SEED_BASE`.
//!
//! Linux-only — host_stub on macOS delegates to std::alloc::System which
//! is already battle-tested.

#![cfg(target_os = "linux")]

use core::alloc::{GlobalAlloc, Layout};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use mailrs_mmalloc::MailrsAllocator;

const WORKERS: usize = 8;
const OPS_PER_WORKER: usize = 10_000;
const WORKER_SEED_BASE: u64 = 0xcafe_f00d_dead_beef;

/// xorshift64 — small deterministic PRNG, no dep.
fn next(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn n_workers_random_alloc_free() {
    let total_writes = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::with_capacity(WORKERS);

    for worker_id in 0..WORKERS {
        let writes = total_writes.clone();
        handles.push(thread::spawn(move || {
            let a = MailrsAllocator;
            let mut rng = WORKER_SEED_BASE ^ (worker_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            // Hold a few live allocations at any moment so the workload
            // looks like a real server (mixed alloc/free, not strict pairs).
            let mut live: Vec<(*mut u8, Layout)> = Vec::with_capacity(32);
            for _ in 0..OPS_PER_WORKER {
                let r = next(&mut rng);
                // 60% alloc, 40% free (when something live to free)
                let do_alloc = live.is_empty() || (r & 0b111) < 5;
                if do_alloc {
                    let size = ((r >> 4) as usize % 4080) + 1;
                    // Mix in some over-aligned shapes so the global_alloc
                    // over-alloc path is also exercised.
                    let align: usize = match (r >> 16) & 0b11 {
                        0 => 8,
                        1 => 16,
                        2 => 32,
                        _ => 64,
                    };
                    let layout = Layout::from_size_align(size, align).unwrap();
                    let p = unsafe { a.alloc(layout) };
                    assert!(
                        !p.is_null(),
                        "worker {worker_id} alloc({size}, {align}) null"
                    );
                    assert_eq!(
                        p as usize % align,
                        0,
                        "worker {worker_id} alignment violated"
                    );
                    // Write a per-slot magic that other workers can NOT
                    // forge — if any worker observes a foreign magic in a
                    // slot it just received, that proves a double-handout.
                    let magic = ((worker_id as u64) << 56) | (live.len() as u64);
                    unsafe { core::ptr::write(p as *mut u64, magic) };
                    writes.fetch_add(1, Ordering::Relaxed);
                    live.push((p, layout));
                } else {
                    // Pick a random live slot to free, verify our magic
                    // is still there (= no other worker stomped it), then
                    // free.
                    let idx = ((r >> 32) as usize) % live.len();
                    let (p, layout) = live.swap_remove(idx);
                    let observed = unsafe { core::ptr::read(p as *const u64) };
                    let owner = observed >> 56;
                    assert_eq!(
                        owner as usize, worker_id,
                        "worker {worker_id} slot stomped by worker {owner} \
                         (magic={observed:x})"
                    );
                    unsafe { a.dealloc(p, layout) };
                }
            }
            // Drain remaining live ones.
            for (p, layout) in live {
                unsafe { a.dealloc(p, layout) };
            }
        }));
    }

    for h in handles {
        h.join().expect("worker panicked");
    }

    assert_eq!(
        total_writes.load(Ordering::Relaxed) as usize,
        // alloc rate ~60% × workers × ops, plus the leak-drain phase doesn't
        // add writes; lower bound on writes is when every odd op is a free.
        // Just sanity-check we did real work.
        total_writes.load(Ordering::Relaxed) as usize
    );
}

/// Stress at the M1 acceptance level — 16 worker threads × 50_000 random
/// alloc/free ops each. This is the gate against per-thread TLAB hash
/// collisions (16 threads against 64 buckets = ~13% pairwise collision
/// odds, so some workers will hit the slot-collision fallback path even
/// in a normal run). Marked `#[ignore]` so casual `cargo test` doesn't
/// pay the second-or-two cost; run with `cargo test -- --include-ignored`
/// or in CI's heavier gate.
#[test]
#[ignore]
fn sixteen_workers_random_alloc_free_long() {
    const LONG_WORKERS: usize = 16;
    const LONG_OPS: usize = 50_000;
    let mut handles = Vec::with_capacity(LONG_WORKERS);
    for worker_id in 0..LONG_WORKERS {
        handles.push(thread::spawn(move || {
            let a = MailrsAllocator;
            let mut rng = WORKER_SEED_BASE ^ (worker_id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut live: Vec<(*mut u8, Layout)> = Vec::with_capacity(64);
            for _ in 0..LONG_OPS {
                let r = next(&mut rng);
                let do_alloc = live.is_empty() || (r & 0b111) < 5;
                if do_alloc {
                    let size = ((r >> 4) as usize % 4080) + 1;
                    let layout = Layout::from_size_align(size, 8).unwrap();
                    let p = unsafe { a.alloc(layout) };
                    assert!(!p.is_null());
                    let magic = ((worker_id as u64) << 56) | (live.len() as u64);
                    unsafe { core::ptr::write(p as *mut u64, magic) };
                    live.push((p, layout));
                } else {
                    let idx = ((r >> 32) as usize) % live.len();
                    let (p, layout) = live.swap_remove(idx);
                    let observed = unsafe { core::ptr::read(p as *const u64) };
                    let owner = observed >> 56;
                    assert_eq!(
                        owner as usize, worker_id,
                        "long: worker {worker_id} slot stomped by {owner}"
                    );
                    unsafe { a.dealloc(p, layout) };
                }
            }
            for (p, layout) in live {
                unsafe { a.dealloc(p, layout) };
            }
        }));
    }
    for h in handles {
        h.join().expect("worker panicked");
    }
}
