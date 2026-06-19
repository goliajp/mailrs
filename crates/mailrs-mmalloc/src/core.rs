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

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::central::CentralQueue;
use crate::large::{large_alloc, large_free};
use crate::size_class::{Allocator, PER_CLASS_CAP, PerClassStats, SIZE_CLASSES};
use crate::span::SPAN_LEN;
use crate::thread_cache;
use crate::tlab::{TLAB_CACHE_DEPTH, TlabCache};
use mailrs_syscall::gettid;

// ============================================================
// SpanRegistry — open-addressed hash, O(1) ptr→span lookup
// ============================================================

/// Max spans tracked by `SpanRegistry`. = `PER_CLASS_CAP *
/// SIZE_CLASSES.len()`. Matches the upper bound the underlying
/// `size_class::Allocator` can reach plus large-alloc entries (which
/// share the same registry).
pub const MAX_REGISTERED_SPANS: usize = PER_CLASS_CAP * SIZE_CLASSES.len();

/// Hash table capacity. Load factor cap 0.5 — 2× the max population
/// so probe chains stay short. Power-of-two so we can mask instead of
/// modulo.
const REGISTRY_CAPACITY: usize = (MAX_REGISTERED_SPANS * 2).next_power_of_two();

/// Sentinel class index marking a large (mmap-direct) allocation
/// rather than a small-span slot.
pub const LARGE_CLASS_IDX: u8 = u8::MAX;

/// `log2(SPAN_LEN)` — used to derive the hash key for span-base entries.
/// Spans are SPAN_LEN-aligned (see `Span::new_for_class`), so the low
/// SPAN_LEN_LOG2 bits of a span base are zero; using `base >>
/// SPAN_LEN_LOG2` as the hash input throws away the redundant bits.
const SPAN_LEN_LOG2: u32 = SPAN_LEN.trailing_zeros();

#[derive(Clone, Copy)]
struct RegistryEntry {
    /// Base address of the registered region. `0` marks a vacant slot;
    /// `usize::MAX` marks a tombstone (removed slot, probe must
    /// continue past it). Neither sentinel can collide with a real
    /// mmap result.
    base: usize,
    /// Size class index — `0..SIZE_CLASSES.len()` for a small span,
    /// `LARGE_CLASS_IDX` for a large mmap-direct allocation.
    class_idx: u8,
    /// For small spans: index into `Allocator::classes[class_idx]`
    /// (the per-class span array slot the `Span` object lives in).
    /// Lets `Allocator::dealloc` skip the linear scan and jump straight
    /// to the owning span. Unused for large allocs.
    idx_in_class: u16,
    /// Region size in bytes. Small span: `SPAN_LEN`. Large alloc:
    /// page-rounded user size. Used by Layer 0 `free` to route to
    /// `large_free` and by Layer 1 small-free to recover the slot
    /// size class.
    size: usize,
}

const VACANT_ENTRY: RegistryEntry = RegistryEntry {
    base: 0,
    class_idx: 0,
    idx_in_class: 0,
    size: 0,
};

/// Open-addressed hash table for ptr → owning region lookup. Linear
/// probing; load factor capped at 0.5. Single-writer (callers hold
/// `CORE_LOCK`), so no internal synchronisation.
///
/// Lookup is O(1) amortized: aligned-mmap guarantees every span
/// starts on a SPAN_LEN boundary, so the lookup key is
/// `span_base = ptr & !(SPAN_LEN - 1)` — derivable from any interior
/// pointer with one bitwise AND. The hash function is a multiplicative
/// hash on `base >> SPAN_LEN_LOG2`, then mask to the table size.
pub struct SpanRegistry {
    table: [RegistryEntry; REGISTRY_CAPACITY],
    /// Live entry count. Used only for the `len`/`is_empty`
    /// diagnostics — the hash table itself never needs it to operate.
    live: u32,
}

impl Default for SpanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

const fn hash_key(base: usize) -> usize {
    // Fibonacci hashing on the span-aligned key. Multiplier is
    // `(2^64 / golden_ratio)` rounded — gives a near-uniform distribution
    // for sequential or clustered inputs without a slow modulo. Mask to
    // the table size below.
    let key = base >> SPAN_LEN_LOG2;
    key.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

impl SpanRegistry {
    pub const fn new() -> Self {
        SpanRegistry {
            table: [VACANT_ENTRY; REGISTRY_CAPACITY],
            live: 0,
        }
    }

    /// Heap-allocate a fresh `SpanRegistry`. The static-sized hash
    /// table is several MB — `Box::new(SpanRegistry::boxed())` first
    /// constructs it on the stack then moves into the box, which
    /// overflows the 2 MB default thread stack. `boxed()` uses
    /// `alloc_zeroed` directly so the registry never lives on the
    /// stack. The zero bit pattern IS a valid `SpanRegistry` (all
    /// entries `VACANT_ENTRY = 0`, live = 0), so `alloc_zeroed` is
    /// sound.
    ///
    /// Mainly for tests; production uses the `static mut CORE_REGISTRY`
    /// directly so this is never on the hot path.
    pub fn boxed() -> Box<Self> {
        use core::alloc::Layout;
        let layout = Layout::new::<Self>();
        // SAFETY: `Self` is zero-valid (all VACANT_ENTRY = 0); `alloc_zeroed`
        // returns a pointer to layout.size() bytes of zeroed memory.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) } as *mut Self;
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        unsafe { Box::from_raw(ptr) }
    }

    /// Insert a region. `base` must be unique (a span / large alloc
    /// is only registered once); the call returns `false` if the
    /// table is at the 0.5 load-factor cap (= MAX_REGISTERED_SPANS
    /// live entries) — in practice never hit because the underlying
    /// Allocator's per-class cap fires first.
    pub fn insert(&mut self, base: usize, class_idx: u8, size: usize) -> bool {
        self.insert_with_idx(base, class_idx, 0, size)
    }

    /// Insert with an explicit `idx_in_class` hint for Layer 1 fast
    /// dispatch. Layer 0 (`insert`) uses 0; the caller can pass the
    /// actual idx for the small-span hot path.
    pub fn insert_with_idx(
        &mut self,
        base: usize,
        class_idx: u8,
        idx_in_class: u16,
        size: usize,
    ) -> bool {
        if self.live as usize >= MAX_REGISTERED_SPANS {
            return false;
        }
        let mut slot = hash_key(base) & (REGISTRY_CAPACITY - 1);
        loop {
            let entry = &self.table[slot];
            // Vacant or tombstone → use this slot. Updating an existing
            // base is also allowed (idempotent re-register).
            if entry.base == 0 || entry.base == usize::MAX || entry.base == base {
                let was_live = entry.base != 0 && entry.base != usize::MAX;
                self.table[slot] = RegistryEntry {
                    base,
                    class_idx,
                    idx_in_class,
                    size,
                };
                if !was_live {
                    self.live += 1;
                }
                return true;
            }
            slot = (slot + 1) & (REGISTRY_CAPACITY - 1);
        }
    }

    /// Lookup `ptr` (anywhere inside a registered region) → `(class_idx,
    /// size)`. Returns `None` if `ptr` falls outside every registered
    /// region.
    ///
    /// O(1) for small spans (SPAN_LEN-aligned via `aligned_mmap`).
    /// Large allocs are also SPAN_LEN-aligned by accident (every mmap
    /// is page-aligned) so they're discoverable by the same
    /// `ptr & mask` for any ptr that falls within the page-rounded
    /// region. For large allocs that may straddle SPAN_LEN boundaries
    /// (size > SPAN_LEN), `lookup` falls back to a multi-probe by
    /// recomputing the candidate base at each SPAN_LEN boundary
    /// inside a small horizon.
    pub fn lookup(&self, ptr: usize) -> Option<(u8, usize)> {
        // Primary attempt: single-SPAN_LEN-aligned base.
        let base = ptr & !(SPAN_LEN - 1);
        if let Some(entry) = self.probe(base)
            && ptr >= entry.base
            && ptr < entry.base + entry.size
        {
            return Some((entry.class_idx, entry.size));
        }
        // Large alloc fallback: the alloc may have started at a
        // SPAN_LEN-aligned base K spans below ptr's bucket. Walk back
        // up to a small horizon (8 SPAN_LENs = 4 MB for 512K SPAN).
        // Caller can grow this if real workloads need it.
        for shift in 1..8 {
            let candidate = base.wrapping_sub(shift * SPAN_LEN);
            if let Some(entry) = self.probe(candidate)
                && entry.class_idx == LARGE_CLASS_IDX
                && ptr >= entry.base
                && ptr < entry.base + entry.size
            {
                return Some((entry.class_idx, entry.size));
            }
        }
        None
    }

    /// Same as `lookup` but additionally returns `idx_in_class`, used
    /// by Layer 1 small-free fast path to jump straight to the owning
    /// `Allocator::classes[class_idx][idx]`.
    pub fn lookup_full(&self, ptr: usize) -> Option<(u8, u16, usize)> {
        let base = ptr & !(SPAN_LEN - 1);
        if let Some(entry) = self.probe(base)
            && ptr >= entry.base
            && ptr < entry.base + entry.size
        {
            return Some((entry.class_idx, entry.idx_in_class, entry.size));
        }
        None
    }

    /// Remove the entry whose base is `base` (for small spans: the
    /// SPAN_LEN-aligned start; for large allocs: the page-aligned
    /// mmap result). Returns `Some((class_idx, size))` on success,
    /// `None` if no matching entry. Leaves a tombstone in the slot
    /// so probe chains for other entries still terminate correctly.
    pub fn remove(&mut self, base: usize) -> Option<(u8, usize)> {
        let mut slot = hash_key(base) & (REGISTRY_CAPACITY - 1);
        loop {
            let entry = self.table[slot];
            if entry.base == 0 {
                // Vacant — search ends.
                return None;
            }
            if entry.base == base {
                self.table[slot] = RegistryEntry {
                    base: usize::MAX, // tombstone
                    class_idx: 0,
                    idx_in_class: 0,
                    size: 0,
                };
                self.live -= 1;
                return Some((entry.class_idx, entry.size));
            }
            slot = (slot + 1) & (REGISTRY_CAPACITY - 1);
        }
    }

    /// Probe for the entry with `base` exactly. Returns the entry by
    /// value (Copy) so the borrow is short. Skips tombstones, stops at
    /// vacant.
    #[inline]
    fn probe(&self, base: usize) -> Option<RegistryEntry> {
        let mut slot = hash_key(base) & (REGISTRY_CAPACITY - 1);
        loop {
            let entry = self.table[slot];
            if entry.base == 0 {
                return None;
            }
            if entry.base == base {
                return Some(entry);
            }
            slot = (slot + 1) & (REGISTRY_CAPACITY - 1);
        }
    }

    /// Current live entry count.
    #[inline]
    pub fn len(&self) -> usize {
        self.live as usize
    }

    /// True iff no entries registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
}

// ============================================================
// Global core allocator — owns Allocator + SpanRegistry pair
// ============================================================

static CORE_LOCK: AtomicBool = AtomicBool::new(false);
static mut CORE_ALLOC: Allocator = Allocator::new();
static mut CORE_REGISTRY: SpanRegistry = SpanRegistry::new();

// ---- M6 observability counters ---------------------------------
//
// Lifetime cumulative counters bumped on every successful alloc/free.
// `Relaxed` ordering is fine — these counters are monotonic, never
// read for synchronisation; consumers just want eventual visibility.
// One atomic add per alloc/free is ~5 cycles — measurable but small
// relative to the lock + Span work on the cold path.

static SMALL_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static SMALL_FREE_COUNT: AtomicU64 = AtomicU64::new(0);
static LARGE_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static LARGE_FREE_COUNT: AtomicU64 = AtomicU64::new(0);
/// Live (currently-mapped) large-alloc bytes — incremented in
/// `alloc_sized`'s large path, decremented in `free_sized`'s.
static LARGE_OUTSTANDING_BYTES: AtomicUsize = AtomicUsize::new(0);
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

/// Process-wide central queue. Lock-free MPMC stack per size class
/// (Treiber-stack push/pop with tagged-pointer ABA defence — see
/// `central.rs`). Acts as the slot-routing buffer between per-thread
/// TLABs:
/// - TLAB overflow on free → `CORE_CENTRAL.push` (lock-free; no
///   `CORE_LOCK`)
/// - TLAB miss on alloc → `CORE_CENTRAL.pop` first; only fall through
///   to the locked Allocator if Central is also empty
/// - Cross-thread free routing happens here naturally: thread A's
///   overflow pushes to Central, thread B's miss pops from it. No
///   coordination needed beyond the AtomicU64 head.
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

/// Refill batch size — number of slots `refill_tlab` pulls from the
/// central allocator under one `CORE_LOCK` acquisition. Amortizes lock
/// cost across `BATCH` subsequent TLAB hits. 8 chosen empirically as a
/// compromise: large enough to give meaningful amortization, small
/// enough that a cold class doesn't pre-grow more spans than the
/// caller really needs.
const REFILL_BATCH: usize = 8;

/// Allocate one slot from the central allocator + register the span in
/// `CORE_REGISTRY` (with idx_in_class hint) if a new span was grown.
/// Caller MUST hold `CORE_LOCK`. Returns `null_mut()` on OOM.
///
/// With M2's aligned-mmap + hash-registry, the registered `span_base`
/// is the just-handed-out `p` (first slot in a fresh span is at offset
/// 0, AND `Span::new_for_class` now uses `mmap_anon_rw_aligned` so
/// `p == span_base` and `p & (SPAN_LEN - 1) == 0`). The
/// `idx_in_class` from `AllocOutcome` lets subsequent free hits skip
/// `Allocator::dealloc`'s linear scan via `dealloc_hinted`.
#[inline]
unsafe fn alloc_one_under_lock(class_idx: usize, size: usize) -> *mut u8 {
    let outcome = match unsafe { (*&raw mut CORE_ALLOC).alloc_with_idx(size) } {
        Some(o) => o,
        None => return core::ptr::null_mut(),
    };
    if outcome.grew_span {
        let base = outcome.ptr as usize;
        debug_assert!(
            base & (SPAN_LEN - 1) == 0,
            "alloc_one_under_lock: grown span base {base:#x} not SPAN_LEN-aligned"
        );
        unsafe {
            (*&raw mut CORE_REGISTRY).insert_with_idx(
                base,
                class_idx as u8,
                outcome.idx_in_class,
                SPAN_LEN,
            );
        }
    }
    outcome.ptr
}

/// Free one slot back to the central allocator using the registry's
/// O(1) `(class_idx, idx_in_class)` hint to skip
/// `Allocator::dealloc`'s linear scan. Caller MUST hold `CORE_LOCK`.
///
/// On M4: if the free empties the owning span, `dealloc_hinted`
/// `madvise(MADV_DONTNEED)`'s the span pages — VMA stays mapped, RSS
/// drops, registry entry stays valid (the span object lives on in
/// `Allocator::classes[][]`, ready for instant reuse without re-mmap).
/// So no registry remove on shrink; the entry is permanent for the
/// span's process lifetime.
#[inline]
unsafe fn free_one_under_lock(ptr: *mut u8, size: usize) {
    let class_idx = match Allocator::bucket_for(size) {
        Some(c) => c,
        None => return,
    };
    let idx = match unsafe { (*&raw const CORE_REGISTRY).lookup_full(ptr as usize) } {
        Some((found_class, idx, _)) if found_class as usize == class_idx => idx,
        _ => {
            // Registry miss or class mismatch — fall back to the
            // legacy scan in case the alloc somehow bypassed the
            // registry. Shouldn't fire on Layer 1 allocs that always
            // go through `alloc_one_under_lock`.
            let _decommitted = unsafe { (*&raw mut CORE_ALLOC).dealloc(ptr, size) };
            return;
        }
    };
    let _decommitted = unsafe { (*&raw mut CORE_ALLOC).dealloc_hinted(ptr, class_idx, idx) };
}

/// Refill an empty TLAB class. Strategy:
///
/// 1. **Try `CORE_CENTRAL.pop` up to `REFILL_BATCH` times — no lock.**
///    Central is the lock-free cross-thread slot buffer. If another
///    thread recently overflowed slots of this class, they're sitting
///    here waiting. First `pop` returns the caller's slot; subsequent
///    pops fill the TLAB.
/// 2. **If Central is empty, fall back to `CORE_LOCK + Allocator`.**
///    Pull up to `REFILL_BATCH - already_filled` slots from the
///    central allocator under one lock acquisition.
///
/// # Safety
///
/// `tlab` must point to a `TlabCache` whose owning thread is the
/// caller (= the per-thread-cache slot's `owner_tid == gettid()`).
#[inline(never)]
unsafe fn refill_tlab(tlab: *mut TlabCache, class_idx: usize, size: usize) -> *mut u8 {
    let mut result = core::ptr::null_mut::<u8>();
    let mut filled = 0usize;
    // Phase 1 — drain Central into TLAB. Lock-free, may amortize the
    // entire refill against zero lock acquisitions in steady state.
    while filled < REFILL_BATCH {
        let Some(p) = CORE_CENTRAL.pop(class_idx) else {
            break;
        };
        if filled == 0 {
            result = p;
        } else if !unsafe { (*tlab).push(class_idx, p) } {
            // TLAB full — give back to Central so the slot isn't lost.
            unsafe { CORE_CENTRAL.push(class_idx, p) };
            break;
        }
        filled += 1;
    }
    if filled == REFILL_BATCH {
        return result;
    }
    // Phase 2 — Central exhausted, dip into the locked Allocator for
    // the remainder.
    lock();
    while filled < REFILL_BATCH {
        let p = unsafe { alloc_one_under_lock(class_idx, size) };
        if p.is_null() {
            break;
        }
        if filled == 0 {
            result = p;
        } else if !unsafe { (*tlab).push(class_idx, p) } {
            unsafe { free_one_under_lock(p, size) };
            break;
        }
        filled += 1;
    }
    unlock();
    result
}

/// Flush half the TLAB class **to `CORE_CENTRAL`** (lock-free push)
/// plus the caller's incoming slot. Used when a free arrives and the
/// TLAB class is already at `TLAB_CACHE_DEPTH`. By draining to
/// Central rather than the locked Allocator, this path becomes fully
/// lock-free — perfect for cross-thread free routing (the slots will
/// be popped by whichever thread next misses its TLAB on this class).
///
/// # Safety
///
/// `tlab` must point to a TLAB owned by the caller. `ptr` must be a
/// valid slot pointer of `size` bytes, not already freed.
#[inline(never)]
unsafe fn flush_tlab_and_push(tlab: *mut TlabCache, class_idx: usize, ptr: *mut u8, size: usize) {
    let _ = size; // Central doesn't need size (slots are class-typed)
    // Drain half the TLAB to Central. Leaves room for the incoming
    // push plus headroom for the next free.
    let target = TLAB_CACHE_DEPTH / 2;
    for _ in 0..target {
        let Some(p) = (unsafe { (*tlab).pop(class_idx) }) else {
            break;
        };
        unsafe { CORE_CENTRAL.push(class_idx, p) };
    }
    // Push the caller's slot. The TLAB now has room (we just drained
    // half) so this should always succeed; if it doesn't (concurrent
    // drain raced us — can't happen for a single-thread-owned TLAB,
    // but guard anyway), fall back to Central.
    if !unsafe { (*tlab).push(class_idx, ptr) } {
        unsafe { CORE_CENTRAL.push(class_idx, ptr) };
    }
}

/// Layer 1 alloc — caller knows size. Hot path; `free_sized` skips
/// registry. Returns NULL on OOM, sentinel on `size == 0`.
///
/// Fast path (~99% of calls in steady state):
/// 1. `gettid()` syscall (~30 ns)
/// 2. Per-thread cache `try_claim` — single atomic load on the owned
///    bucket, no CAS after initial claim
/// 3. `TlabCache::pop` — single load + single store, no atomics
///
/// Total: ~50-100 cycles for the hit, NO `CORE_LOCK` acquisition.
///
/// Miss paths:
/// - TLAB empty for class → `refill_tlab` pulls `REFILL_BATCH=8` slots
///   from central under one `CORE_LOCK`; subsequent 7 allocs hit the
///   TLAB fast path
/// - Thread hashed to a slot another thread owns (collision; rare with
///   typical worker counts vs `THREAD_SLOTS=64`) → fall straight to
///   central under `CORE_LOCK`
/// - Large request (> 4 KB) → direct mmap path, registry insert under
///   `CORE_LOCK`; TLAB not involved
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
        LARGE_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        LARGE_OUTSTANDING_BYTES.fetch_add(rounded, Ordering::Relaxed);
        return p;
    }
    let class_idx = match Allocator::bucket_for(size) {
        Some(i) => i,
        None => return core::ptr::null_mut(),
    };
    SMALL_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let tid = gettid();
    if let Some(tlab) = thread_cache::try_claim(tid) {
        // Fast path — owned slot, no atomics inside the TLAB.
        if let Some(p) = unsafe { (*tlab).pop(class_idx) } {
            return p;
        }
        // Cold — TLAB empty for this class. Refill batch under lock.
        return unsafe { refill_tlab(tlab, class_idx, size) };
    }
    // Collision — fall straight to central under lock.
    lock();
    let p = unsafe { alloc_one_under_lock(class_idx, size) };
    unlock();
    p
}

/// Layer 1 free — caller provides original size. Skips registry
/// lookup entirely (fastest path).
///
/// Fast path: push the freed slot into the calling thread's TLAB.
/// `gettid()` + `try_claim` + `TlabCache::push` — no `CORE_LOCK`.
///
/// Miss paths:
/// - TLAB class already at `TLAB_CACHE_DEPTH` → `flush_tlab_and_push`
///   drains half the TLAB back to central under one `CORE_LOCK`
/// - Slot collision (another thread owns this thread's TLAB bucket)
///   → direct central dealloc under `CORE_LOCK`
/// - Large free (> 4 KB) → registry remove + munmap under `CORE_LOCK`
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized` with
/// the matching `size`, not already freed.
#[inline(always)]
pub unsafe fn free_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr == zero_sentinel() || size == 0 {
        return;
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        // Large path — deregister from registry then munmap.
        let rounded = (size.max(1) + 4095) & !4095;
        lock();
        unsafe { (*&raw mut CORE_REGISTRY).remove(ptr as usize) };
        unlock();
        let _ = unsafe { large_free(ptr, size) };
        LARGE_FREE_COUNT.fetch_add(1, Ordering::Relaxed);
        LARGE_OUTSTANDING_BYTES.fetch_sub(rounded, Ordering::Relaxed);
        return;
    }
    let class_idx = match Allocator::bucket_for(size) {
        Some(i) => i,
        None => return,
    };
    SMALL_FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    let tid = gettid();
    if let Some(tlab) = thread_cache::try_claim(tid) {
        if unsafe { (*tlab).push(class_idx, ptr) } {
            return;
        }
        // TLAB full for this class — flush under lock + free this slot.
        unsafe { flush_tlab_and_push(tlab, class_idx, ptr, size) };
        return;
    }
    // Collision — central dealloc under lock.
    lock();
    unsafe { free_one_under_lock(ptr, size) };
    unlock();
}

// ============================================================
// M6 — observability
// ============================================================

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
        let r = SpanRegistry::boxed();
        assert!(r.lookup(0x1000).is_none());
        assert!(r.is_empty());
    }

    #[test]
    fn registry_insert_then_lookup_in_range() {
        let mut r = SpanRegistry::boxed();
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
        let mut r = SpanRegistry::boxed();
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
        let mut r = SpanRegistry::boxed();
        r.insert(0x5_0000_0000, 1, SPAN_LEN);
        assert!(r.lookup(0x1_0000_0000).is_none());
    }

    #[test]
    fn registry_remove_drops_entry() {
        let mut r = SpanRegistry::boxed();
        let bases = [0x1_0000_0000usize, 0x3_0000_0000, 0x5_0000_0000];
        for (i, b) in bases.iter().enumerate() {
            assert!(r.insert(*b, i as u8, SPAN_LEN));
        }
        assert_eq!(r.len(), 3);
        // Remove middle entry — `remove` takes the registered span
        // BASE (not an interior ptr); the M2 hash registry resolves
        // an exact base lookup, not a containment search.
        let (class_idx, size) = r.remove(0x3_0000_0000).expect("remove middle");
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
        let mut r = SpanRegistry::boxed();
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
        // populated by alloc_sized's grow hook. Keep a second slot
        // live so the free below doesn't fully empty the span (which
        // would trigger shrink + unmap, and the next alloc would
        // come from a brand-new span at a different base).
        let p = alloc_sized(128);
        let keep = alloc_sized(128);
        assert!(!p.is_null());
        assert!(!keep.is_null());
        unsafe {
            *p = 0xcd;
            free(p);
        }
        // Subsequent alloc of same size should reuse the freed
        // slot (Span freelist is LIFO; span survived because `keep`
        // is still live).
        let p2 = alloc_sized(128);
        assert_eq!(p, p2, "Layer 0 free didn't return slot to span");
        unsafe {
            free_sized(p2, 128);
            free_sized(keep, 128);
        }
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
