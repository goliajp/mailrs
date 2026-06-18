//! Central queue — lock-free MPMC remote-free dispatch.
//!
//! Phase 2c item 9 of the metal-tier allocator redesign per
//! `docs/v0.7-A2-finding.md`. Per-class lock-free MPMC stack
//! (Treiber-style) that holds slots freed by a non-owning thread,
//! to be drained back into the owning TLAB on next alloc-on-miss.
//!
//! Hot path role (Phase 2c item 10 integration):
//! - Free from owning thread → TLAB.push (Phase 2b item 7)
//! - Free from foreign thread → CentralQueue.push (Phase 2c item 10)
//! - Alloc TLAB miss → CentralQueue.drain to refill TLAB, then
//!   Allocator.alloc on still-empty
//!
//! Single-threaded runtime today does not exercise the foreign-
//! free path; CentralQueue ships scaffolded + tested but not
//! integrated until item 10. Multi-thread runtime post-v1.0
//! lights up the path without further redesign.
//!
//! Lock-free invariants:
//! - Push: CAS retry loop on `head` (Treiber stack push).
//! - Pop: CAS retry on `head`, reading `next` of the
//!   speculatively-popped node. Caller may safely deref the
//!   returned ptr only after the CAS succeeds.
//! - **ABA hazard** documented + accepted for v1: a producer can
//!   re-push the same node after a consumer's load-of-next, but
//!   in this design freed slots cycle through the allocator
//!   before being re-handed-out, so ABA requires N>1 cores all
//!   freeing+reallocating the same address in lockstep — not a
//!   workload mmalloc targets. Phase 2c+ upgrade options: tagged
//!   pointer with high-16-bit counter (aarch64 + x86_64 both have
//!   16 unused top bits on 64-bit addresses), or hazard pointers.
//!
//! Reference: Treiber 1986 "Systems Programming: Coping with
//! Parallelism" (IBM Research Report RJ 5118).

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::size_class::SIZE_CLASSES;

/// Embedded freelist node — overlays an unused slot's first 8
/// bytes (same trick as Span freelist). When a slot is in the
/// CentralQueue, its first 8 bytes hold the pointer to the next
/// queued slot (or null at the end).
#[repr(C)]
struct CentralNode {
    next: *mut CentralNode,
}

/// Per-class lock-free MPMC stack for remote-free dispatch.
pub struct CentralQueue {
    heads: [AtomicPtr<CentralNode>; SIZE_CLASSES.len()],
}

impl Default for CentralQueue {
    fn default() -> Self {
        Self::new()
    }
}

const NULL_HEAD: AtomicPtr<CentralNode> = AtomicPtr::new(ptr::null_mut());

impl CentralQueue {
    pub const fn new() -> Self {
        CentralQueue {
            heads: [NULL_HEAD; SIZE_CLASSES.len()],
        }
    }

    /// Push a slot pointer into the remote queue for `class_idx`.
    /// Lock-free CAS retry loop.
    ///
    /// # Safety
    ///
    /// `ptr` must be a slot pointer owned by this allocator (= came
    /// from `Allocator::alloc` of a slot in the matching size class)
    /// and not concurrently in use elsewhere (= TLAB, central queue,
    /// or caller hand-out). Writes to `ptr`'s first 8 bytes for the
    /// embedded `next` link.
    pub unsafe fn push(&self, class_idx: usize, ptr: *mut u8) {
        let node = ptr as *mut CentralNode;
        let head = &self.heads[class_idx];
        let mut cur = head.load(Ordering::Acquire);
        loop {
            unsafe {
                (*node).next = cur;
            }
            match head.compare_exchange_weak(cur, node, Ordering::Release, Ordering::Acquire) {
                Ok(_) => return,
                Err(now) => cur = now,
            }
        }
    }

    /// Pop one slot for `class_idx`. Returns `None` if queue is
    /// empty. Lock-free CAS retry loop.
    ///
    /// ABA hazard: documented at mod level. Caller must understand
    /// that under heavy multi-thread churn the returned pointer
    /// may be the "second incarnation" of an address that was
    /// pushed→popped→re-pushed during the loop. Safe in mmalloc
    /// because re-push only happens after a full allocator
    /// roundtrip (free → drain to TLAB → re-alloc → re-free →
    /// re-push), which cannot complete in the window of a single
    /// `pop` attempt.
    pub fn pop(&self, class_idx: usize) -> Option<*mut u8> {
        let head = &self.heads[class_idx];
        let mut cur = head.load(Ordering::Acquire);
        loop {
            if cur.is_null() {
                return None;
            }
            // SAFETY: cur was loaded from `head` which was set by a
            // prior `push`; push wrote `next` before publishing
            // `cur` as head. The read here happens-before the CAS
            // success below; if CAS fails, we retry with the new
            // head and re-read its `next`.
            let next = unsafe { (*cur).next };
            match head.compare_exchange_weak(cur, next, Ordering::Release, Ordering::Acquire) {
                Ok(_) => return Some(cur as *mut u8),
                Err(now) => cur = now,
            }
        }
    }

    /// Drain all currently-queued slots for `class_idx`, invoking
    /// `f(ptr)` for each. Used by TLAB refill on miss to batch-
    /// pull remote-freed slots back into the owning thread's TLAB.
    ///
    /// Note: drain is not atomic — new pushes that race the drain
    /// may land before or after the snapshot; both outcomes are
    /// safe (drain returns either Some-from-old-push or None and
    /// the new push remains in the queue for the next drain).
    pub fn drain<F: FnMut(*mut u8)>(&self, class_idx: usize, mut f: F) {
        while let Some(p) = self.pop(class_idx) {
            f(p);
        }
    }

    /// Snapshot check — true iff `class_idx`'s head is currently
    /// null. Cheap (single atomic load); useful for "TLAB refill
    /// needed, check central first" fast path.
    #[inline]
    pub fn is_empty(&self, class_idx: usize) -> bool {
        self.heads[class_idx].load(Ordering::Acquire).is_null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// Test-only slot pool: backing storage for raw pointers so we
    /// can push them into the queue. Each "slot" is a Box<[u8; 16]>
    /// (16 bytes for the embedded next pointer + padding).
    struct SlotPool {
        slots: Vec<Box<[u8; 16]>>,
    }

    impl SlotPool {
        fn new(n: usize) -> Self {
            let mut slots = Vec::with_capacity(n);
            for _ in 0..n {
                slots.push(Box::new([0u8; 16]));
            }
            SlotPool { slots }
        }

        fn ptr(&mut self, i: usize) -> *mut u8 {
            self.slots[i].as_mut_ptr()
        }
    }

    #[test]
    fn fresh_queue_is_empty() {
        let q = CentralQueue::new();
        for c in 0..SIZE_CLASSES.len() {
            assert!(q.is_empty(c));
            assert!(q.pop(c).is_none());
        }
    }

    #[test]
    fn single_push_pop_returns_same_ptr() {
        let q = CentralQueue::new();
        let mut pool = SlotPool::new(1);
        let p = pool.ptr(0);
        unsafe { q.push(3, p) };
        assert!(!q.is_empty(3));
        assert_eq!(q.pop(3), Some(p));
        assert!(q.is_empty(3));
    }

    #[test]
    fn multi_push_pop_lifo() {
        let q = CentralQueue::new();
        let mut pool = SlotPool::new(4);
        let ptrs: Vec<_> = (0..4).map(|i| pool.ptr(i)).collect();
        for &p in &ptrs {
            unsafe { q.push(1, p) };
        }
        // LIFO: last pushed comes out first.
        for &p in ptrs.iter().rev() {
            assert_eq!(q.pop(1), Some(p));
        }
        assert!(q.is_empty(1));
    }

    #[test]
    fn classes_are_independent() {
        let q = CentralQueue::new();
        let mut pool = SlotPool::new(3);
        let (p0, p1, p2) = (pool.ptr(0), pool.ptr(1), pool.ptr(2));
        unsafe {
            q.push(0, p0);
            q.push(4, p1);
            q.push(8, p2);
        }
        assert_eq!(q.pop(0), Some(p0));
        assert_eq!(q.pop(4), Some(p1));
        assert_eq!(q.pop(8), Some(p2));
        for c in [0, 4, 8] {
            assert!(q.is_empty(c));
        }
    }

    #[test]
    fn drain_iterates_all() {
        let q = CentralQueue::new();
        let mut pool = SlotPool::new(5);
        let ptrs: Vec<_> = (0..5).map(|i| pool.ptr(i)).collect();
        for &p in &ptrs {
            unsafe { q.push(2, p) };
        }
        let mut seen = vec![];
        q.drain(2, |p| seen.push(p));
        assert_eq!(seen.len(), 5);
        for p in &ptrs {
            assert!(seen.contains(p));
        }
        assert!(q.is_empty(2));
    }

    /// Concurrent stress: N producer threads push K slots each,
    /// M consumer threads pop until total = N*K. Validates lock-
    /// free invariants under contention.
    #[test]
    fn concurrent_push_pop_no_loss() {
        const N_PRODUCERS: usize = 4;
        const N_CONSUMERS: usize = 4;
        const PER_THREAD: usize = 100;
        const TOTAL: usize = N_PRODUCERS * PER_THREAD;

        // Pre-allocate all slots so we can pass raw ptrs across threads.
        let pool: Vec<Box<[u8; 16]>> = (0..TOTAL).map(|_| Box::new([0u8; 16])).collect();
        let ptrs: Vec<usize> = pool.iter().map(|b| b.as_ptr() as usize).collect();
        let q = Arc::new(CentralQueue::new());

        // Producers.
        let mut handles = vec![];
        for prod_i in 0..N_PRODUCERS {
            let q = q.clone();
            let chunk: Vec<usize> = ptrs[prod_i * PER_THREAD..(prod_i + 1) * PER_THREAD].to_vec();
            handles.push(thread::spawn(move || {
                for p in chunk {
                    unsafe { q.push(0, p as *mut u8) };
                }
            }));
        }

        // Consumers — pop until we see all TOTAL slots.
        let popped = Arc::new(std::sync::Mutex::new(Vec::<usize>::with_capacity(TOTAL)));
        let consumer_done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for _ in 0..N_CONSUMERS {
            let q = q.clone();
            let popped = popped.clone();
            let done = consumer_done.clone();
            handles.push(thread::spawn(move || {
                loop {
                    if let Some(p) = q.pop(0) {
                        let mut g = popped.lock().unwrap();
                        g.push(p as usize);
                        if g.len() >= TOTAL {
                            return;
                        }
                    } else if done.load(Ordering::Acquire) >= N_PRODUCERS {
                        // Producers all finished and queue empty.
                        if q.is_empty(0) {
                            return;
                        }
                    } else {
                        std::thread::yield_now();
                    }
                }
            }));
        }

        // Wait for producers then signal consumers.
        for h in handles.drain(..N_PRODUCERS) {
            h.join().unwrap();
        }
        consumer_done.store(N_PRODUCERS, Ordering::Release);
        for h in handles {
            h.join().unwrap();
        }

        let g = popped.lock().unwrap();
        assert_eq!(g.len(), TOTAL, "lost slots under concurrency");
        // Verify no duplicates (= no ABA-like double-pop)
        let mut sorted: Vec<usize> = g.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), TOTAL, "duplicate pops detected");
    }
}
