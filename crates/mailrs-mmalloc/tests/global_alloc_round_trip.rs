//! Integration test — drive `MailrsAllocator` through the GlobalAlloc trait
//! the way Rust std actually uses it (Box / Vec / String shapes), but as a
//! local value rather than the process's `#[global_allocator]` so we can
//! exercise it from inside a test binary that already has its own (System)
//! global allocator.
//!
//! Linux-only — the macOS host_stub just delegates to `std::alloc::System`,
//! so re-testing that here would be a no-op.

#![cfg(target_os = "linux")]

use core::alloc::{GlobalAlloc, Layout};

use mailrs_mmalloc::MailrsAllocator;

/// Tiny single alloc round-trip — covers the size_class fast path with the
/// most common Rust shape: small `Box<u64>` ≈ Layout (8, 8).
#[test]
fn box_u64_round_trip() {
    let a = MailrsAllocator;
    let layout = Layout::new::<u64>();
    unsafe {
        let p = a.alloc(layout) as *mut u64;
        assert!(!p.is_null());
        assert_eq!(p as usize % 8, 0, "must be at least u64-aligned");
        *p = 0xdead_beef_cafe_babe;
        assert_eq!(*p, 0xdead_beef_cafe_babe);
        a.dealloc(p as *mut u8, layout);
    }
}

/// Vec-like shape — alloc, write a pattern across the whole slab, dealloc.
/// Exercises every size class up to the small/large boundary (4096) plus the
/// large path one step above.
#[test]
fn vec_u8_each_class() {
    let a = MailrsAllocator;
    for &len in &[
        1usize, 7, 16, 31, 64, 256, 1024, 4096, 4097, 16_384, 1_048_576,
    ] {
        let layout = Layout::array::<u8>(len).unwrap();
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null(), "alloc({len}) returned null");
            // Touch every byte to prove the region is writable end-to-end.
            for off in 0..len {
                *p.add(off) = (off & 0xff) as u8;
            }
            for off in 0..len {
                assert_eq!(*p.add(off), (off & 0xff) as u8, "len={len} off={off}");
            }
            a.dealloc(p, layout);
        }
    }
}

/// `String::from` / `Vec::with_capacity` pattern — alloc_zeroed.
/// Must hand back a region that's actually zero (not "happens to be zero
/// because mmap zeros pages") — write a pattern, dealloc, alloc_zeroed
/// the same size, verify it's zero. With size_class recycling the second
/// alloc is likely to return the same slot, so the zeroing must overwrite
/// the prior pattern.
#[test]
fn alloc_zeroed_actually_zeroes() {
    let a = MailrsAllocator;
    let layout = Layout::array::<u8>(256).unwrap();
    unsafe {
        let p1 = a.alloc(layout);
        for off in 0..256 {
            *p1.add(off) = 0xaa;
        }
        a.dealloc(p1, layout);
        let p2 = a.alloc_zeroed(layout);
        for off in 0..256 {
            assert_eq!(*p2.add(off), 0, "alloc_zeroed left byte off={off} non-zero");
        }
        a.dealloc(p2, layout);
    }
}

/// `Vec::push` past capacity → realloc growth. The contents of the old
/// buffer must survive verbatim into the new buffer.
#[test]
fn realloc_grow_preserves_bytes() {
    let a = MailrsAllocator;
    let old_layout = Layout::array::<u8>(64).unwrap();
    let new_size = 1024;
    unsafe {
        let p = a.alloc(old_layout);
        for off in 0..64 {
            *p.add(off) = (off + 1) as u8;
        }
        let p2 = a.realloc(p, old_layout, new_size);
        assert!(!p2.is_null());
        for off in 0..64 {
            assert_eq!(
                *p2.add(off),
                (off + 1) as u8,
                "realloc lost byte at off={off}"
            );
        }
        let new_layout = Layout::from_size_align(new_size, old_layout.align()).unwrap();
        a.dealloc(p2, new_layout);
    }
}

/// `Vec::shrink_to_fit` → realloc shrink. Caller's bytes 0..new must survive.
#[test]
fn realloc_shrink_preserves_bytes() {
    let a = MailrsAllocator;
    let old_layout = Layout::array::<u8>(1024).unwrap();
    let new_size = 64;
    unsafe {
        let p = a.alloc(old_layout);
        for off in 0..1024 {
            *p.add(off) = (off & 0xff) as u8;
        }
        let p2 = a.realloc(p, old_layout, new_size);
        assert!(!p2.is_null());
        for off in 0..new_size {
            assert_eq!(
                *p2.add(off),
                (off & 0xff) as u8,
                "shrink lost byte at off={off}"
            );
        }
        let new_layout = Layout::from_size_align(new_size, old_layout.align()).unwrap();
        a.dealloc(p2, new_layout);
    }
}

/// Over-aligned requests (align > 16) take the over-alloc + header path.
/// Verify the returned pointer is actually aligned and the round-trip is
/// dealloc-safe (no corruption of the header).
#[test]
fn over_aligned_alloc() {
    let a = MailrsAllocator;
    for &(size, align) in &[
        (64usize, 32usize),
        (128, 64),
        (256, 128),
        (512, 256),
        (1024, 512),
    ] {
        let layout = Layout::from_size_align(size, align).unwrap();
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null(), "alloc({size}, {align}) returned null");
            assert_eq!(p as usize % align, 0, "alloc({size}, {align}) not aligned");
            // Write across the user-visible range — must NOT touch the
            // header byte at (p - HEADER).
            for off in 0..size {
                *p.add(off) = 0xbb;
            }
            a.dealloc(p, layout);
        }
    }
}

/// Many small ones, many large ones, interleaved. Catches any class /
/// large-path dispatch bug that the single-shot tests above might miss
/// because each runs against a "clean" allocator state.
#[test]
fn interleaved_small_and_large() {
    let a = MailrsAllocator;
    let mut live = Vec::with_capacity(1024);
    let mut rng: u64 = 0xfeed_face_dead_c0de;
    for _ in 0..1024 {
        // xorshift64 — deterministic, no rand dep.
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        // Mix of small (16..4096) and a few large (4097..65536).
        let size = if (rng & 0xf) == 0 {
            ((rng >> 4) as usize % 60_000) + 4097
        } else {
            ((rng >> 4) as usize % 4080) + 16
        };
        let layout = Layout::from_size_align(size, 8).unwrap();
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null(), "alloc({size}) null");
            *p = 0xab;
            live.push((p, layout));
        }
    }
    for (p, layout) in live {
        unsafe { a.dealloc(p, layout) };
    }
}
