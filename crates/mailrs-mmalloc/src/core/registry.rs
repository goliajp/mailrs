//! `SpanRegistry` — the open-addressed table that maps an interior
//! pointer back to the span that owns it.

use crate::size_class::{PER_CLASS_CAP, SIZE_CLASSES};
use crate::span::SPAN_LEN;

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
