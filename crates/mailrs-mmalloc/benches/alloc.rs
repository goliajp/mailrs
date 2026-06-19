//! Criterion bench surface for `mailrs-mmalloc`. M0 ships the harness +
//! baseline measurements; M7 layers on `vs glibc` and `vs mimalloc` axes
//! and populates `BUDGETS.md` with per-op p95 numbers + 3× CI headroom.
//!
//! On non-Linux the host_stub forwards to `std::alloc::System`, so the
//! benches still compile but their numbers say nothing about mmalloc;
//! cfg-gate the actual benches accordingly.

use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(target_os = "linux")]
mod linux_benches {
    use super::*;
    use core::alloc::{GlobalAlloc, Layout};
    use criterion::black_box;
    use mailrs_mmalloc::MailrsAllocator;

    /// Single-thread small-class alloc + immediate free. Measures the
    /// fast-path round-trip cost — the headline number an allocator is
    /// judged on.
    pub fn alloc_free_per_class(c: &mut Criterion) {
        let a = MailrsAllocator;
        let mut group = c.benchmark_group("alloc_free_small_per_class");
        for &size in &[16usize, 64, 256, 1024, 4096] {
            group.bench_with_input(format!("size={size}"), &size, |b, &size| {
                let layout = Layout::from_size_align(size, 8).unwrap();
                b.iter(|| unsafe {
                    let p = a.alloc(layout);
                    *p = 0xab;
                    black_box(p);
                    a.dealloc(p, layout);
                });
            });
        }
        group.finish();
    }

    /// Large-path: > 4 KB → direct mmap fallback. Each iteration pays
    /// one mmap + one munmap, so this measures syscall-bound cost.
    pub fn alloc_free_large(c: &mut Criterion) {
        let a = MailrsAllocator;
        let mut group = c.benchmark_group("alloc_free_large");
        for &size in &[8 * 1024usize, 64 * 1024, 1024 * 1024] {
            group.bench_with_input(format!("size={size}"), &size, |b, &size| {
                let layout = Layout::from_size_align(size, 4096).unwrap();
                b.iter(|| unsafe {
                    let p = a.alloc(layout);
                    *p = 0xab;
                    black_box(p);
                    a.dealloc(p, layout);
                });
            });
        }
        group.finish();
    }

    /// Realloc growth: 64 B → 1 MB doubling. Stresses the realloc path
    /// (alloc new + copy + dealloc old per step).
    pub fn realloc_growth(c: &mut Criterion) {
        let a = MailrsAllocator;
        c.bench_function("realloc_growth_64_to_1m", |b| {
            b.iter(|| unsafe {
                let mut layout = Layout::from_size_align(64, 8).unwrap();
                let mut p = a.alloc(layout);
                let mut size = 64usize;
                while size < 1024 * 1024 {
                    let new_size = size * 2;
                    p = a.realloc(p, layout, new_size);
                    layout = Layout::from_size_align(new_size, 8).unwrap();
                    size = new_size;
                }
                *p = 0xab;
                black_box(p);
                a.dealloc(p, layout);
            });
        });
    }

    /// Hold-N pattern — keep N allocations live, churn the (N+1)th.
    /// Mirrors a server workload that has steady working set + edge churn.
    /// The first alloc of each iteration is cheap (recycled slot); only
    /// the steady-state alloc-then-free dominates.
    pub fn churn_with_live_n(c: &mut Criterion) {
        let a = MailrsAllocator;
        let mut group = c.benchmark_group("churn_with_live_set");
        for &n in &[8usize, 64, 512] {
            group.bench_with_input(format!("live_n={n}"), &n, |b, &n| {
                let layout = Layout::from_size_align(256, 8).unwrap();
                let mut live: Vec<*mut u8> = (0..n).map(|_| unsafe { a.alloc(layout) }).collect();
                b.iter(|| unsafe {
                    let p = a.alloc(layout);
                    *p = 0xab;
                    black_box(p);
                    a.dealloc(p, layout);
                });
                for p in live.drain(..) {
                    unsafe { a.dealloc(p, layout) };
                }
            });
        }
        group.finish();
    }
}

#[cfg(target_os = "linux")]
criterion_group!(
    benches,
    linux_benches::alloc_free_per_class,
    linux_benches::alloc_free_large,
    linux_benches::realloc_growth,
    linux_benches::churn_with_live_n
);

#[cfg(not(target_os = "linux"))]
fn host_stub_placeholder(_c: &mut Criterion) {
    // host_stub forwards to System; nothing meaningful to bench.
}

#[cfg(not(target_os = "linux"))]
criterion_group!(benches, host_stub_placeholder);

criterion_main!(benches);
