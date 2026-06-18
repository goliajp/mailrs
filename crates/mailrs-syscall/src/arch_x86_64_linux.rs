//! x86_64 Linux syscall trampoline.
//!
//! ABI summary (from `man syscall`):
//! - syscall number → `rax`
//! - args → `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` (note `r10`, not `rcx`)
//! - trap instruction → `syscall`
//! - return value → `rax` (negative in `[-4095, -1]` indicates `-errno`)
//! - `rcx` and `r11` are clobbered by the kernel as part of the
//!   `syscall` instruction's contract (the kernel uses them to
//!   stash the user-space `RIP` / `RFLAGS`).
//!
//! We tell rustc about every register the kernel may touch via
//! `clobber_abi("C")` plus the explicit `rcx` / `r11` listed in
//! `lateout`/`out`. Forgetting either is a recipe for a CFI miss /
//! NULL-deref in a downstream caller after the syscall returns.

use core::arch::asm;

/// 6-argument raw syscall on x86_64 Linux.
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
            "syscall",
            inlateout("rax") sysno as i64 => ret,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            in("r10") a3,
            in("r8")  a4,
            in("r9")  a5,
            lateout("rcx") _,
            lateout("r11") _,
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
