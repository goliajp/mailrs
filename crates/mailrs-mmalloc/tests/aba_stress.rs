//! ABA stress for the tagged-pointer `CentralQueue`.
//!
//! M3 swapped the Treiber stack's `AtomicPtr` for an `AtomicU64` packed
//! as `(counter: u16 << 48) | (ptr: u48)`. The counter increments on
//! every push or pop, so even if the same slot address gets popped +
//! re-pushed inside another thread's CAS-retry window, the head's
//! packed value differs and the CAS correctly fails+retries.
//!
//! This test exercises the worst case: a small pool of slots
//! continuously cycling through push → pop → push on the same class
//! across many threads. Pre-tagged-pointer, this would produce
//! duplicate pops (the same slot handed to two consumers) under heavy
//! contention; with tags, it must NOT.
//!
//! Linux-only because `CentralQueue` is `#[cfg(target_os = "linux")]`.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use mailrs_mmalloc::central::CentralQueue;

const THREADS: usize = 8;
const OPS_PER_THREAD: usize = 100_000;
const POOL_SIZE: usize = 16;

/// Tight push/pop loop on a single class with a small pool of recycled
/// slots — the classical ABA scenario. Each push or pop increments the
/// tagged-head counter, so even with the same physical address cycled
/// repeatedly, the consumer's CAS sees a different head value when the
/// counter advances and correctly retries instead of swapping in stale
/// state.
#[test]
fn tagged_pointer_eliminates_aba() {
    let q = Arc::new(CentralQueue::new());
    // Pre-allocate a pool of slot-shaped buffers. Each "slot" is 16
    // bytes so the embedded `next` pointer fits.
    let pool: Vec<Box<[u8; 16]>> = (0..POOL_SIZE).map(|_| Box::new([0u8; 16])).collect();
    let ptrs: Vec<usize> = pool.iter().map(|b| b.as_ptr() as usize).collect();
    // Seed Central with the pool.
    for p in &ptrs {
        unsafe { q.push(0, *p as *mut u8) };
    }

    let pop_count = Arc::new(AtomicUsize::new(0));
    let push_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let q = q.clone();
        let pop_count = pop_count.clone();
        let push_count = push_count.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..OPS_PER_THREAD {
                // Pop one (spin if empty)
                let p = loop {
                    if let Some(p) = q.pop(0) {
                        break p;
                    }
                    std::thread::yield_now();
                };
                pop_count.fetch_add(1, Ordering::Relaxed);
                // Re-push immediately so other threads have something
                // to pop too.
                unsafe { q.push(0, p) };
                push_count.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for h in handles {
        h.join().expect("worker panicked");
    }

    let total = THREADS * OPS_PER_THREAD;
    assert_eq!(pop_count.load(Ordering::Relaxed), total);
    assert_eq!(push_count.load(Ordering::Relaxed), total);

    // Final pool size must equal what we seeded — no slot lost, no
    // slot duplicated. Drain Central and check.
    let mut final_pool: Vec<usize> = Vec::new();
    while let Some(p) = q.pop(0) {
        final_pool.push(p as usize);
    }
    final_pool.sort_unstable();
    let mut expected = ptrs.clone();
    expected.sort_unstable();
    assert_eq!(
        final_pool, expected,
        "ABA: pool diverged from seed (duplicate or lost slot)"
    );

    // Keep `pool` alive to the end so the slot pointers stayed valid.
    drop(pool);
}
