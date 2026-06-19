//! Core API — Layer 0 / Layer 1 metal-tier allocator surface.
//!
//! Phase 2a items 3+4 of the metal-tier redesign per
//! `docs/v0.7-A2-finding.md`. Provides:
//!
//! - **Layer 0** — `alloc(size)` / `free(ptr)`. `free(ptr)`
//!   consults `SpanRegistry` for ptr→class lookup; matches libc-
//!   shape contract WITHOUT per-alloc SHIM_HEADER. The ptr→span
//!   info lives in span metadata (one entry per span, not one per
//!   alloc) — header overhead amortizes by `slot_count`.
//! - **Layer 1** — `alloc_sized(size)` / `free_sized(ptr, size)`.
//!   Caller-knows-size fast path; skips `SpanRegistry` lookup
//!   entirely.
//!
//! Both layers share the underlying `size_class::Allocator`
//! (Phase 2a item 2 span-backed shape). Layer 0 free is the only
//! path that pays the lookup cost; sub-crate hot paths will use
//! Layer 1 once IR codegen migrates (Phase 2e).
//!
//! Phase 2a item 5 will migrate `extern_api`'s `__torajs_malloc` /
//! `__torajs_free` to wrap these layers; `__torajs_libc_*` shim
//! becomes Layer 2 wrapping Layer 1 (SHIM_HEADER retained only in
//! Layer 2 for external C consumers whose API truly lost size).
//!
//! Phase 2c will upgrade `SpanRegistry` to a per-CPU sharded
//! hashmap with O(1) lookup; the current binary-search form is
//! O(log n) — already orders better than the size_class fallback
//! O(n) scan path and adequate for Phase 2a/2b workloads.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::central::CentralQueue;
use crate::large::{large_alloc, large_free};
use crate::size_class::{Allocator, PER_CLASS_CAP, SIZE_CLASSES};
use crate::span::SPAN_LEN;
use crate::tlab::TlabCache;

// ============================================================
// SpanRegistry — ptr→span O(log n) lookup
// ============================================================

/// Max spans tracked by `SpanRegistry`. = `PER_CLASS_CAP *
/// SIZE_CLASSES.len()`. Matches the upper bound the underlying
/// `size_class::Allocator` can reach plus Phase 2d large-alloc
/// entries (which share the same array — large allocs are
/// infrequent enough that the shared cap isn't tight). Phase 2c
/// sharded hashmap removes this cap.
pub const MAX_REGISTERED_SPANS: usize = PER_CLASS_CAP * SIZE_CLASSES.len();

/// Sentinel class index marking a large (mmap-direct) allocation
/// rather than a small-span slot. Phase 2d item 11+12.
pub const LARGE_CLASS_IDX: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct RegistryEntry {
    /// Base address of the registered region (mmap'd start).
    base: usize,
    /// Size class index — 0..SIZE_CLASSES.len() for small span,
    /// `LARGE_CLASS_IDX` for large mmap-direct allocations.
    class_idx: u8,
    /// Region size in bytes — used for ptr-containment check
    /// (small span: SPAN_LEN; large alloc: PAGE_4K-rounded size).
    /// Carrying it per-entry lets `lookup` and `remove` uniformly
    /// handle both small + large without branching on class_idx.
    size: usize,
}

const ZERO_ENTRY: RegistryEntry = RegistryEntry {
    base: 0,
    class_idx: 0,
    size: 0,
};

pub struct SpanRegistry {
    /// Sorted by `base` ascending. First `cur` entries occupied;
    /// remainder is zeroed but unread. Sort invariant maintained
    /// by `insert`.
    entries: [RegistryEntry; MAX_REGISTERED_SPANS],
    cur: u32,
}

impl Default for SpanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanRegistry {
    pub const fn new() -> Self {
        SpanRegistry {
            entries: [ZERO_ENTRY; MAX_REGISTERED_SPANS],
            cur: 0,
        }
    }

    /// Insert a new region entry. Maintains sorted-by-base
    /// invariant via insertion sort. O(n) but called only on
    /// span grow or large alloc (both rare — amortized cost
    /// negligible per-alloc).
    /// Returns `false` if the registry is at cap.
    pub fn insert(&mut self, base: usize, class_idx: u8, size: usize) -> bool {
        let cur = self.cur as usize;
        if cur >= MAX_REGISTERED_SPANS {
            return false;
        }
        // Binary search for insertion point in [0, cur).
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base < base {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let insert_at = lo;
        // Shift entries [insert_at..cur] right by 1.
        let mut i = cur;
        while i > insert_at {
            self.entries[i] = self.entries[i - 1];
            i -= 1;
        }
        self.entries[insert_at] = RegistryEntry {
            base,
            class_idx,
            size,
        };
        self.cur += 1;
        true
    }

    /// Lookup `ptr` → `(class_idx, size)`. O(log n) via binary
    /// search on sorted-by-base entries.
    ///
    /// Returns `None` if `ptr` falls outside any registered region.
    /// For Phase 2d large-alloc dispatch: returned `class_idx` is
    /// `LARGE_CLASS_IDX` for mmap-direct large allocs; `size` is
    /// then the mmap'd size to pass to `large_free`.
    pub fn lookup(&self, ptr: usize) -> Option<(u8, usize)> {
        let cur = self.cur as usize;
        if cur == 0 {
            return None;
        }
        // Find largest i in [0, cur) where entries[i].base <= ptr.
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base <= ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let entry = &self.entries[lo - 1];
        if ptr >= entry.base && ptr < entry.base + entry.size {
            Some((entry.class_idx, entry.size))
        } else {
            None
        }
    }

    /// Remove the entry whose region contains `ptr`. O(log n +
    /// shift). Returns `Some((class_idx, size))` of the removed
    /// region, or `None` if `ptr` is not in any registered region.
    /// Used by Phase 2d large-alloc free path to deregister
    /// before `large_free`'s munmap.
    pub fn remove(&mut self, ptr: usize) -> Option<(u8, usize)> {
        let cur = self.cur as usize;
        if cur == 0 {
            return None;
        }
        // Binary search for containing entry.
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base <= ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let idx = lo - 1;
        let entry = self.entries[idx];
        if ptr < entry.base || ptr >= entry.base + entry.size {
            return None;
        }
        // Shift [idx+1..cur) left by 1.
        for i in idx..(cur - 1) {
            self.entries[i] = self.entries[i + 1];
        }
        self.cur -= 1;
        Some((entry.class_idx, entry.size))
    }

    /// Current span population count.
    #[inline]
    pub fn len(&self) -> usize {
        self.cur as usize
    }

    /// True iff no spans registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cur == 0
    }
}

// ============================================================
// Global core allocator — owns Allocator + SpanRegistry pair
// ============================================================

static CORE_LOCK: AtomicBool = AtomicBool::new(false);
static mut CORE_ALLOC: Allocator = Allocator::new();
static mut CORE_REGISTRY: SpanRegistry = SpanRegistry::new();
// Step 16-c-2 (2026-05-29): downgraded from `#[thread_local]` to a
// plain `static mut` to drop the last `__tlv_bootstrap` undefined
// symbol from user binaries (A5 zero-libc-undef goal). On macOS
// aarch64 `#[thread_local]` forces a `$tlv$init` / `__tlv_bootstrap`
// dyld dependency — see docs/v0.7-A5-finding.md. The single-threaded
// runtime has no concurrent observer, so a process-wide TLAB is sound.
//
// Access via `&raw mut` like CORE_ALLOC / CORE_REGISTRY above (clears
// the edition-2024 `static_mut_refs` lint). `TlabCache::new()` is
// const — the static initializes at compile time, no ctor.
//
// MULTI-THREAD RE-DERIVATION (v0.8 backlog): a process-wide TLAB
// defeats the per-thread isolation a threaded runtime needs. When the
// first threaded path lands (Promise/async/worker), re-derive per-
// thread TLABs via a syscall-thread-id-indexed manual array (NOT
// `#[thread_local]` — Darwin local-exec TLS still routes via tlv).
//
// `#[unsafe(no_mangle)] pub` (Phase 2e item 13): stable symbol name
// so the toolchain can inline TLAB.pop/push at alloc/free sites
// (LLVM-era backend did; the native ARM64 re-port is swap-3+
// backlog — see cmd_build's synthesize_obj_alloc).
//
// mailrs-fork note: `__mailrs_core_tlab` is NOT touched by
// `alloc_sized` / `free_sized` on this fork. mailrs-server is a
// tokio multi-worker binary, and unsynchronized pop/push to a
// process-wide TLAB would be a data race. The hot path bypasses
// the TLAB, dispatching free → `CORE_CENTRAL.push` (lock-free MPMC)
// and alloc → `CORE_CENTRAL.pop`. The TLAB stays in the tree so a
// future per-thread upgrade (gettid-indexed array, see Phase 2c
// backlog above) can re-light it without re-introducing the
// symbol.
#[unsafe(no_mangle)]
pub static mut __mailrs_core_tlab: TlabCache = TlabCache::new();

/// Process-wide central queue (Phase 2c item 10). Lock-free MPMC
/// stack per size class; acts as the TLAB overflow buffer + cross-
/// thread free landing zone. Single-thread runtime today: TLAB
/// overflow → Central.push (lock-free, faster than Allocator.dealloc's
/// O(n_spans) scan); alloc TLAB.miss → drain Central back to TLAB
/// before falling through to Allocator.alloc. Multi-thread future:
/// foreign-thread free → Central.push automatically routes to
/// owning thread's next refill cycle.
///
/// mailrs fork: held statically per upstream-sync policy but unused
/// by `alloc_sized` / `free_sized` (see those functions' comments
/// for the ABA-window rationale that took it off the hot path).
#[allow(dead_code)]
static CORE_CENTRAL: CentralQueue = CentralQueue::new();

#[inline]
fn lock() {
    while CORE_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
fn unlock() {
    CORE_LOCK.store(false, Ordering::Release);
}

/// Sentinel returned on zero-size alloc to keep callers from
/// confusing with NULL=OOM. Matches glibc behavior.
#[inline]
fn zero_sentinel() -> *mut u8 {
    &raw const CORE_LOCK as *mut u8
}

// ============================================================
// Layer 1 — alloc_sized / free_sized (hot path, no lookup)
// ============================================================

/// Layer 1 alloc — caller knows size. Hot path; `free_sized`
/// skips registry. Returns NULL on OOM, sentinel on `size == 0`.
///
/// mailrs fork: everything goes through `CORE_ALLOC.alloc` under
/// `CORE_LOCK`. The upstream design's lock-free `CentralQueue` +
/// per-thread TLAB shortcuts were both unsafe in this binary:
/// - TLAB is a process-wide `static mut` with no synchronisation,
///   which races on tokio's multi-worker runtime.
/// - CentralQueue is a Treiber stack whose ABA defence assumes the
///   freed-slot transit time through the TLAB is long enough that
///   a popped node can't be re-pushed during a single CAS attempt.
///   Without the TLAB the ABA window collapses to a few instructions
///   and produces silent pointer aliasing under multi-thread contention.
/// One spin-lock per alloc/free is correct and adequate for a
/// baseline; future work re-introduces TLAB via gettid-indexed
/// per-thread state once the surface is reliable.
///
/// `#[inline(always)]` (Phase 2e item 13a): lets fat LTO + cc -flto
/// inline the hot path into user-binary IR, eliminating extern "C"
/// call overhead per alloc.
#[inline(always)]
pub fn alloc_sized(size: usize) -> *mut u8 {
    if size == 0 {
        return zero_sentinel();
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        // Large path — direct mmap + registry insert so Layer 0
        // free(ptr) can recover size for `large_free` dispatch.
        let p = match large_alloc(size) {
            Ok(p) => p,
            Err(_) => return core::ptr::null_mut(),
        };
        // large_alloc rounds size up to PAGE_4K internally; mirror
        // here so the registered size matches the mmap'd region's
        // actual length (needed for ptr-containment lookup).
        let rounded = (size.max(1) + 4095) & !4095;
        lock();
        unsafe { (*&raw mut CORE_REGISTRY).insert(p as usize, LARGE_CLASS_IDX, rounded) };
        unlock();
        return p;
    }
    let class_idx = match Allocator::bucket_for(size) {
        Some(i) => i,
        None => return core::ptr::null_mut(),
    };
    lock();
    let before_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    let p = unsafe { (*&raw mut CORE_ALLOC).alloc(size) }.unwrap_or(core::ptr::null_mut());
    let after_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    if !p.is_null() && after_mapped > before_mapped {
        // Span base = ptr rounded down to SPAN_LEN boundary.
        let span_base = (p as usize) & !(SPAN_LEN - 1);
        unsafe {
            (*&raw mut CORE_REGISTRY).insert(span_base, class_idx as u8, SPAN_LEN);
        }
    }
    unlock();
    p
}

/// Layer 1 free — caller provides original size. Skips registry
/// lookup entirely (fastest path). Routes through the locked
/// central `Allocator.dealloc`; see `alloc_sized`'s comment for why
/// neither the TLAB nor the lock-free Central is used on this fork.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized`
/// with the matching `size`, not already freed.
#[inline(always)]
pub unsafe fn free_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr == zero_sentinel() || size == 0 {
        return;
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        // Large path — deregister from registry then munmap.
        lock();
        unsafe { (*&raw mut CORE_REGISTRY).remove(ptr as usize) };
        unlock();
        let _ = unsafe { large_free(ptr, size) };
        return;
    }
    lock();
    unsafe { (*&raw mut CORE_ALLOC).dealloc(ptr, size) };
    unlock();
}

// ============================================================
// Layer 0 — alloc / free (size recovered from registry)
// ============================================================

/// Layer 0 alloc — same shape as `alloc_sized` (size is always
/// known by the caller in any sane API). Kept as a distinct symbol
/// for surface-symmetry with `free` (which does need registry).
#[inline]
pub fn alloc(size: usize) -> *mut u8 {
    alloc_sized(size)
}

/// Layer 0 free — caller has no size. SpanRegistry lookup
/// recovers size class. O(log n_spans) per free.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized`,
/// not already freed.
pub unsafe fn free(ptr: *mut u8) {
    if ptr.is_null() || ptr == zero_sentinel() {
        return;
    }
    lock();
    let lookup_result = unsafe { (*&raw const CORE_REGISTRY).lookup(ptr as usize) };
    unlock();
    match lookup_result {
        Some((LARGE_CLASS_IDX, large_size)) => {
            // Large alloc — deregister then munmap.
            lock();
            unsafe { (*&raw mut CORE_REGISTRY).remove(ptr as usize) };
            unlock();
            let _ = unsafe { large_free(ptr, large_size) };
        }
        Some((idx, _)) => {
            // Small span — recover size from class.
            let size = SIZE_CLASSES[idx as usize];
            unsafe { free_sized(ptr, size) };
        }
        None => {
            // ptr not in any registered region — was not allocated
            // by this allocator (or already-freed). No-op (matches
            // libc free(NULL) safety contract).
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SpanRegistry direct tests (no global state) ---

    #[test]
    fn registry_lookup_empty_is_none() {
        let r = SpanRegistry::new();
        assert!(r.lookup(0x1000).is_none());
        assert!(r.is_empty());
    }

    #[test]
    fn registry_insert_then_lookup_in_range() {
        let mut r = SpanRegistry::new();
        let base = 0x1_0000_0000usize;
        assert!(r.insert(base, 3, SPAN_LEN));
        // Inside span
        assert_eq!(r.lookup(base), Some((3, SPAN_LEN)));
        assert_eq!(r.lookup(base + SPAN_LEN / 2), Some((3, SPAN_LEN)));
        assert_eq!(r.lookup(base + SPAN_LEN - 1), Some((3, SPAN_LEN)));
        // Outside span
        assert_eq!(r.lookup(base - 1), None);
        assert_eq!(r.lookup(base + SPAN_LEN), None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn registry_insert_maintains_sorted_invariant() {
        let mut r = SpanRegistry::new();
        // Insert in reverse-base order; lookups should still work.
        let bases = [
            0x9_0000_0000usize,
            0x3_0000_0000,
            0x7_0000_0000,
            0x1_0000_0000,
            0x5_0000_0000,
        ];
        for (i, b) in bases.iter().enumerate() {
            assert!(r.insert(*b, i as u8, SPAN_LEN));
        }
        for (i, b) in bases.iter().enumerate() {
            assert_eq!(r.lookup(*b), Some((i as u8, SPAN_LEN)));
            assert_eq!(r.lookup(*b + SPAN_LEN / 2), Some((i as u8, SPAN_LEN)));
        }
        // Lookup between spans returns None.
        assert_eq!(r.lookup(0x2_0000_0000), None);
        assert_eq!(r.lookup(0x4_0000_0000), None);
    }

    #[test]
    fn registry_lookup_below_lowest_is_none() {
        let mut r = SpanRegistry::new();
        r.insert(0x5_0000_0000, 1, SPAN_LEN);
        assert!(r.lookup(0x1_0000_0000).is_none());
    }

    #[test]
    fn registry_remove_drops_entry() {
        let mut r = SpanRegistry::new();
        let bases = [0x1_0000_0000usize, 0x3_0000_0000, 0x5_0000_0000];
        for (i, b) in bases.iter().enumerate() {
            assert!(r.insert(*b, i as u8, SPAN_LEN));
        }
        assert_eq!(r.len(), 3);
        // Remove middle entry.
        let (class_idx, size) = r.remove(0x3_0000_0000 + 100).expect("remove middle");
        assert_eq!(class_idx, 1);
        assert_eq!(size, SPAN_LEN);
        assert_eq!(r.len(), 2);
        // First and last still accessible.
        assert_eq!(r.lookup(0x1_0000_0000), Some((0, SPAN_LEN)));
        assert_eq!(r.lookup(0x5_0000_0000), Some((2, SPAN_LEN)));
        // Removed range lookup returns None.
        assert!(r.lookup(0x3_0000_0000 + 100).is_none());
    }

    #[test]
    fn registry_large_class_tracked() {
        // Phase 2d: LARGE_CLASS_IDX entries with custom size.
        let mut r = SpanRegistry::new();
        let large_base = 0x10_0000_0000usize;
        let large_size = 256 * 1024; // 256 KB large alloc
        assert!(r.insert(large_base, LARGE_CLASS_IDX, large_size));
        assert_eq!(r.lookup(large_base), Some((LARGE_CLASS_IDX, large_size)));
        assert_eq!(
            r.lookup(large_base + large_size - 1),
            Some((LARGE_CLASS_IDX, large_size))
        );
        // Just outside the large region.
        assert_eq!(r.lookup(large_base + large_size), None);
    }

    // --- Layer 1 alloc_sized / free_sized round-trip ---

    #[test]
    fn alloc_sized_returns_nonnull_for_nonzero() {
        let p = alloc_sized(64);
        assert!(!p.is_null(), "alloc 64 returned null");
        unsafe {
            *p = 0xaa;
            assert_eq!(*p, 0xaa);
            free_sized(p, 64);
        }
    }

    #[test]
    fn alloc_sized_zero_returns_sentinel() {
        let p = alloc_sized(0);
        assert!(
            !p.is_null(),
            "zero-size alloc returned null (expected sentinel)"
        );
        // Free of sentinel must be a no-op (not corrupt).
        unsafe { free_sized(p, 0) };
    }

    #[test]
    fn alloc_sized_large_routes_to_large_alloc() {
        // size > biggest size class → large_alloc path.
        let big = SIZE_CLASSES[SIZE_CLASSES.len() - 1] + 1;
        let p = alloc_sized(big);
        assert!(!p.is_null());
        unsafe {
            // Touch first byte; mmap'd region should be writable.
            *p = 0xbb;
            assert_eq!(*p, 0xbb);
            free_sized(p, big);
        }
    }

    // --- Layer 0 free (registry lookup) ---

    #[test]
    fn layer0_free_recovers_size_via_registry() {
        // Layer 1 alloc → Layer 0 free. Registry should have been
        // populated by alloc_sized's grow hook.
        let p = alloc_sized(128);
        assert!(!p.is_null());
        unsafe {
            *p = 0xcd;
            free(p);
        }
        // Subsequent alloc of same size should reuse the freed
        // slot (Span freelist is LIFO).
        let p2 = alloc_sized(128);
        assert_eq!(p, p2, "Layer 0 free didn't return slot to span");
        unsafe { free_sized(p2, 128) };
    }

    #[test]
    fn layer0_free_null_is_safe() {
        unsafe { free(core::ptr::null_mut()) };
    }

    #[test]
    fn layer0_free_sentinel_is_safe() {
        let s = alloc_sized(0);
        unsafe { free(s) };
    }
}
