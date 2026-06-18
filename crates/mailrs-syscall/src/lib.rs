//! mailrs-syscall — minimal raw Linux syscall stubs used by
//! [`mailrs-mmalloc`](../mailrs-mmalloc) to talk to the kernel
//! without going through libc.
//!
//! ## Scope
//!
//! Only the syscalls the in-process mmap-backed allocator needs:
//!
//! - `mmap(NULL, len, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0)`
//!   → `mmap_anon_rw(len)`
//! - `munmap(addr, len)` → `munmap(addr, len)`
//! - `madvise(addr, len, MADV_DONTNEED)` → `madvise_dontneed(addr, len)`
//!   — this is the key bit: tells the kernel we don't need the pages,
//!   the resident pages return to the OS even though the VMA stays
//!   mapped. glibc's per-thread arenas refuse to do this; that
//!   refusal is the proximate cause of mailrs's RSS climb (see
//!   `.claude/notes/rss-leak-attribution-allocator-2026-06-18.md`).
//!
//! All other syscalls — read, write, open, fcntl, gettimeofday … —
//! stay on libc/std for the bulk of mailrs-server; only the
//! allocator path needs the bare syscall, and only because the
//! allocator sits inside `#[global_allocator]` where touching libc
//! during dealloc would deadlock under the global-alloc lock.
//!
//! ## Architectures
//!
//! Linux on x86_64 and aarch64 — both prod targets shipped by
//! `release.yml`. The macOS host build is a stub (`unimplemented!`)
//! so cargo workspace build/test from a dev laptop works; the
//! allocator that calls into this crate is `#[cfg(target_os =
//! "linux")]`-gated, so the stubs are never reached in practice.
//!
//! ## Why 0-dep
//!
//! Per [[reference-torajs-sibling-project]] / `DEPS_AUDIT.md`, mailrs
//! does not import `libc` / `nix` / `rustix` etc. for runtime —
//! every new external crate becomes a security audit surface and an
//! ABI-stability dependency. The torajs project has been driving
//! its own `metal-level` syscall layer for the same reason; this
//! crate is structurally the mailrs side of that pattern, scoped
//! down to the three syscalls the allocator actually needs.

#![allow(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
mod arch_x86_64_linux;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use arch_x86_64_linux::{syscall3, syscall6};

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
mod arch_aarch64_linux;
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
use arch_aarch64_linux::{syscall3, syscall6};

#[cfg(target_os = "linux")]
mod linux_consts {
    /// `mmap` PROT_READ.
    pub const PROT_READ: i64 = 0x01;
    /// `mmap` PROT_WRITE.
    pub const PROT_WRITE: i64 = 0x02;
    /// `mmap` MAP_PRIVATE — copy-on-write.
    pub const MAP_PRIVATE: i64 = 0x02;
    /// `mmap` MAP_ANONYMOUS — no backing file. On Linux this is
    /// `0x20`, unlike macOS (`0x1000`).
    pub const MAP_ANONYMOUS: i64 = 0x20;
    /// `madvise(MADV_DONTNEED)` — release the underlying pages now;
    /// the next access faults a fresh zero page in.
    pub const MADV_DONTNEED: i64 = 4;
}

/// Errno (positive Linux error code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Errno(
    /// Raw POSIX errno value (positive). Zero is never produced; an
    /// `Ok` carries the real return value, an `Err(Errno(n))` carries
    /// the kernel's negative-return mapped back to positive.
    pub i32,
);

#[cfg(target_os = "linux")]
#[inline]
fn decode(raw: i64) -> Result<i64, Errno> {
    // Linux convention: negative return in [-4095, -1] is `-errno`,
    // anything else is the real return value (a pointer or a count).
    if (-4095..0).contains(&raw) {
        Err(Errno((-raw) as i32))
    } else {
        Ok(raw)
    }
}

/// `mmap(NULL, len, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)`.
///
/// Returns a pointer to `len` bytes of fresh, zero-filled,
/// page-aligned memory. The kernel picks the address.
///
/// On macOS host (dev only) this is a stub that returns
/// `Err(Errno(38))` (ENOSYS), so calling sites surface the
/// arch-gating boundary explicitly rather than silently using the
/// wrong allocator path.
#[cfg(target_os = "linux")]
pub fn mmap_anon_rw(len: usize) -> Result<*mut u8, Errno> {
    use linux_consts::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};
    let raw = unsafe {
        syscall6(
            SYS_MMAP,
            0,                      // addr — let kernel pick
            len as i64,             // len
            PROT_READ | PROT_WRITE, // prot
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1, // fd — must be -1 for anon
            0,  // offset
        )
    };
    decode(raw).map(|p| p as *mut u8)
}

/// `munmap(addr, len)` — release the VMA fully. Pages return to the
/// OS unconditionally.
///
/// # Safety
///
/// Caller must pass `(addr, len)` matching a prior `mmap_anon_rw`
/// (or other mmap variant); passing an unrelated address is UB at
/// the kernel level.
#[cfg(target_os = "linux")]
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<(), Errno> {
    let raw = unsafe { syscall3(SYS_MUNMAP, addr as i64, len as i64, 0) };
    decode(raw).map(|_| ())
}

/// `madvise(addr, len, MADV_DONTNEED)` — tell the kernel the caller
/// has no current use for these pages. The VMA stays mapped (so the
/// caller can keep the same address), but the resident pages are
/// returned to the OS immediately and `RSS` for the process drops.
///
/// This is the call glibc per-thread arenas refuse to make, which
/// is why a multi-threaded Rust server bleeds RSS without leaking
/// any logical bytes.
///
/// # Safety
///
/// `(addr, len)` must refer to a region that is mapped and writable;
/// the typical use is on freed size_class blocks within a larger
/// `mmap`'d page.
#[cfg(target_os = "linux")]
pub unsafe fn madvise_dontneed(addr: *mut u8, len: usize) -> Result<(), Errno> {
    let raw = unsafe {
        syscall3(
            SYS_MADVISE,
            addr as i64,
            len as i64,
            linux_consts::MADV_DONTNEED,
        )
    };
    decode(raw).map(|_| ())
}

// ---- per-arch syscall numbers -------------------------------------------

/// `mmap` syscall number on the current target arch. The Linux ABI
/// is **per-arch** (the kernel keeps a stable table per architecture
/// but the numbers differ), so we hard-code the two we support.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const SYS_MMAP: u32 = 9;
/// `munmap` syscall number on x86_64 Linux.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const SYS_MUNMAP: u32 = 11;
/// `madvise` syscall number on x86_64 Linux.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const SYS_MADVISE: u32 = 28;

/// `mmap` syscall number on aarch64 Linux.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const SYS_MMAP: u32 = 222;
/// `munmap` syscall number on aarch64 Linux.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const SYS_MUNMAP: u32 = 215;
/// `madvise` syscall number on aarch64 Linux.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const SYS_MADVISE: u32 = 233;

// ---- macOS host stubs (dev-only) ----------------------------------------
//
// The allocator that calls into this crate is `#[cfg(target_os =
// "linux")]`-gated, so these stubs are never invoked in normal use;
// they exist solely to keep `cargo build --workspace` and
// `cargo test --workspace` green on a macOS developer laptop.

/// macOS host stub — always fails with ENOSYS so an accidental
/// arch-gating slip is loud rather than silently allocating via
/// the wrong path.
#[cfg(not(target_os = "linux"))]
pub fn mmap_anon_rw(_len: usize) -> Result<*mut u8, Errno> {
    Err(Errno(38))
}

/// macOS host stub — always fails with ENOSYS.
///
/// # Safety
///
/// Trivially safe (no-op).
#[cfg(not(target_os = "linux"))]
pub unsafe fn munmap(_addr: *mut u8, _len: usize) -> Result<(), Errno> {
    Err(Errno(38))
}

/// macOS host stub — always fails with ENOSYS.
///
/// # Safety
///
/// Trivially safe (no-op).
#[cfg(not(target_os = "linux"))]
pub unsafe fn madvise_dontneed(_addr: *mut u8, _len: usize) -> Result<(), Errno> {
    Err(Errno(38))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn mmap_munmap_roundtrip() {
        // 4 KiB page; write the first and last byte to prove the
        // mapping is real, then unmap.
        let len = 4096usize;
        let p = mmap_anon_rw(len).expect("mmap");
        assert!(!p.is_null());
        unsafe {
            *p = 0xab;
            *p.add(len - 1) = 0xcd;
            assert_eq!(*p, 0xab);
            assert_eq!(*p.add(len - 1), 0xcd);
        }
        unsafe { munmap(p, len) }.expect("munmap");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn madvise_dontneed_keeps_vma_drops_pages() {
        let len = 4 * 4096usize;
        let p = mmap_anon_rw(len).expect("mmap");
        unsafe {
            // touch every page so the kernel allocates them
            for i in 0..4 {
                *p.add(i * 4096) = 0x42;
            }
            // tell the kernel we don't need them
            madvise_dontneed(p, len).expect("madvise");
            // VMA is still readable — a re-read after MADV_DONTNEED
            // faults a fresh zero page, so the value is now 0
            // (not 0x42 — the kernel discarded the modified page).
            for i in 0..4 {
                assert_eq!(*p.add(i * 4096), 0);
            }
            munmap(p, len).expect("munmap");
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn mac_host_stub_returns_enosys() {
        assert_eq!(mmap_anon_rw(4096).unwrap_err(), Errno(38));
    }
}
