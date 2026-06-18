//! aarch64 Linux syscall trampoline.
//!
//! ABI summary (from `man syscall`):
//! - syscall number → `x8` (Linux convention; NOT `x16` like macOS)
//! - args → `x0..x5` (up to 6 args)
//! - trap instruction → `svc #0`
//! - return value → `x0` (negative in `[-4095, -1]` indicates `-errno`)
//!
//! Unlike the macOS aarch64 path used by torajs-syscall, Linux does
//! not use the carry flag for the error condition — a single negative
//! `x0` is the canonical signal. We don't need the `b.cc / neg`
//! re-encoding dance.

use core::arch::asm;

/// 6-argument raw syscall on aarch64 Linux.
///
/// # Safety
///
/// Caller must pass a valid syscall number and well-formed args.
/// Calling `SYS_MUNMAP` on an address that wasn't `SYS_MMAP`'d is
/// UB at the kernel level.
#[inline]
pub unsafe fn syscall6(sysno: u32, a0: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "svc #0",
            in("x8") sysno as i64,
            inlateout("x0") a0 => ret,
            in("x1") a1,
            in("x2") a2,
            in("x3") a3,
            in("x4") a4,
            in("x5") a5,
            // Linux aarch64 svc clobbers all caller-saved registers
            // (x0..x18) per AArch64 PCS. Without these clobbers, the
            // compiler can park live caller values across the trap
            // and the kernel trashes them. `clobber_abi("C")` covers
            // the PCS caller-saved set; `nostack` keeps `sp`
            // alignment guarantees.
            clobber_abi("C"),
            options(nostack),
        );
    }
    ret
}

/// 3-argument raw syscall — the dominant case (mmap, munmap, madvise…).
///
/// # Safety
///
/// See [`syscall6`].
#[inline]
pub unsafe fn syscall3(sysno: u32, a0: i64, a1: i64, a2: i64) -> i64 {
    unsafe { syscall6(sysno, a0, a1, a2, 0, 0, 0) }
}
