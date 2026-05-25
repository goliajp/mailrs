use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_auth_guard::{AuthCheck, AuthGuard, AuthGuardConfig};
use std::hint::black_box;
use std::net::IpAddr;

fn fresh_guard() -> AuthGuard {
    AuthGuard::new(AuthGuardConfig::default())
}

fn bench_check_empty(c: &mut Criterion) {
    let guard = fresh_guard();
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    c.bench_function("check/empty_map_success_path", |b| {
        b.iter(|| {
            let r = guard.check(black_box(ip), black_box("alice"));
            assert!(matches!(r, AuthCheck::Allowed));
            black_box(r)
        });
    });
}

fn bench_check_after_some_failures(c: &mut Criterion) {
    let guard = fresh_guard();
    let ip: IpAddr = "192.0.2.2".parse().unwrap();
    // 3 failures, below threshold (default 5).
    for _ in 0..3 {
        guard.record_failure(ip, "bob");
    }
    c.bench_function("check/below_threshold_still_allowed", |b| {
        b.iter(|| {
            let r = guard.check(black_box(ip), black_box("bob"));
            black_box(r)
        });
    });
}

fn bench_check_locked_out(c: &mut Criterion) {
    let guard = fresh_guard();
    let ip: IpAddr = "192.0.2.3".parse().unwrap();
    for _ in 0..10 {
        guard.record_failure(ip, "carol");
    }
    c.bench_function("check/locked_out", |b| {
        b.iter(|| {
            let r = guard.check(black_box(ip), black_box("carol"));
            assert!(matches!(r, AuthCheck::LockedOut { .. }));
            black_box(r)
        });
    });
}

fn bench_record_failure_new_key(c: &mut Criterion) {
    c.bench_function("record_failure/fresh_key", |b| {
        b.iter_with_setup(
            || {
                let guard = fresh_guard();
                let ip: IpAddr = "192.0.2.4".parse().unwrap();
                (guard, ip)
            },
            |(guard, ip)| {
                guard.record_failure(ip, black_box("dave"));
                black_box(guard)
            },
        );
    });
}

fn bench_record_failure_repeat(c: &mut Criterion) {
    c.bench_function("record_failure/repeat_same_key", |b| {
        b.iter_with_setup(
            || {
                let guard = fresh_guard();
                let ip: IpAddr = "192.0.2.5".parse().unwrap();
                // prime with one failure
                guard.record_failure(ip, "eve");
                (guard, ip)
            },
            |(guard, ip)| {
                guard.record_failure(ip, black_box("eve"));
                black_box(guard)
            },
        );
    });
}

fn bench_record_success(c: &mut Criterion) {
    c.bench_function("record_success/clears_account_failures", |b| {
        b.iter_with_setup(
            || {
                let guard = fresh_guard();
                let ip: IpAddr = "192.0.2.6".parse().unwrap();
                for _ in 0..3 {
                    guard.record_failure(ip, "frank");
                }
                (guard, ip)
            },
            |(guard, ip)| {
                guard.record_success(ip, black_box("frank"));
                black_box(guard)
            },
        );
    });
}

criterion_group!(
    benches,
    bench_check_empty,
    bench_check_after_some_failures,
    bench_check_locked_out,
    bench_record_failure_new_key,
    bench_record_failure_repeat,
    bench_record_success,
);
criterion_main!(benches);
