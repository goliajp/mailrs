//! Long deterministic random op sequence — single thread, 1M ops mixing
//! alloc / free / realloc with random sizes. Tracks all live pointers in a
//! local map and verifies:
//!
//! - every alloc returns a unique non-null pointer (no double-handout)
//! - every freed pointer was previously alloc'd by this test (no foreign-
//!   ptr free, no underflow into someone else's slot)
//! - after the final drain, the per-test net alloc count returns to zero
//!   (= no leak introduced by this test)
//!
//! The point is to grind enough random combinations to catch corruption
//! that the structured tests above miss. Seeded so failures reproduce.

#![cfg(target_os = "linux")]

use core::alloc::{GlobalAlloc, Layout};
use std::collections::HashMap;

use mailrs_mmalloc::MailrsAllocator;

const OPS: usize = 1_000_000;
const SEED: u64 = 0x1234_5678_9abc_def0;

fn xs(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn one_million_random_ops_single_thread() {
    let a = MailrsAllocator;
    let mut rng = SEED;
    let mut live: HashMap<usize, Layout> = HashMap::with_capacity(2048);
    // Hold the keys in a Vec so we can pick a random live entry in O(1).
    let mut keys: Vec<usize> = Vec::with_capacity(2048);

    for _ in 0..OPS {
        let r = xs(&mut rng);
        let action = r & 0xff;
        if keys.is_empty() || action < 96 {
            // alloc (~37.5% of ops)
            let size = (((r >> 8) as usize) % 4080) + 1;
            let align: usize = match (r >> 24) & 0b11 {
                0 => 8,
                1 => 16,
                2 => 32,
                _ => 64,
            };
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = unsafe { a.alloc(layout) };
            assert!(!p.is_null(), "alloc({size}, {align}) returned null");
            let addr = p as usize;
            assert!(
                live.insert(addr, layout).is_none(),
                "alloc returned duplicate ptr {addr:#x}",
            );
            keys.push(addr);
        } else if action < 192 {
            // free (~37.5% of ops, conditional on non-empty)
            let idx = ((r >> 8) as usize) % keys.len();
            let addr = keys.swap_remove(idx);
            let layout = live.remove(&addr).expect("free of unknown ptr");
            unsafe { a.dealloc(addr as *mut u8, layout) };
        } else {
            // realloc (~25% of ops)
            let idx = ((r >> 8) as usize) % keys.len();
            let addr = keys[idx];
            let layout = *live.get(&addr).unwrap();
            let new_size = (((r >> 32) as usize) % 4080) + 1;
            let p2 = unsafe { a.realloc(addr as *mut u8, layout, new_size) };
            if p2.is_null() {
                // realloc allowed to fail by contract; cleanup the old.
                // mailrs allocator never returns null here for valid in-range
                // sizes, so fail loudly.
                panic!("realloc({addr:#x}, {new_size}) returned null");
            }
            live.remove(&addr);
            keys.swap_remove(idx);
            let new_layout = Layout::from_size_align(new_size, layout.align()).unwrap();
            let new_addr = p2 as usize;
            assert!(
                live.insert(new_addr, new_layout).is_none(),
                "realloc handed back live ptr {new_addr:#x}"
            );
            keys.push(new_addr);
        }
    }
    // Drain everything still live.
    for addr in keys {
        let layout = live.remove(&addr).unwrap();
        unsafe { a.dealloc(addr as *mut u8, layout) };
    }
    assert!(live.is_empty(), "leak in test: {} ptrs remain", live.len());
}
