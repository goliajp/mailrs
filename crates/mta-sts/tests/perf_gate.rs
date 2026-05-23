//! Perf regression gates for the mailrs-mta-sts parsers + enforce fn.
//!
//! Budgets are ~10× release P95 so this catches order-of-magnitude
//! regressions without flaking under load. Debug-mode runs (release.sh
//! `cargo test --workspace`) get a 5× slack on top of that.

use std::time::{Duration, Instant};

use mailrs_mta_sts::{Policy, PolicyMode, StsRecord, enforce, mx_matches};

fn time<F: Fn()>(iterations: u32, f: F) -> Duration {
    for _ in 0..16 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    start.elapsed() / iterations
}

fn budget(release_us: u64, debug_us: u64) -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_micros(debug_us)
    } else {
        Duration::from_micros(release_us)
    }
}

#[test]
fn parse_sts_record_under_budget() {
    let txt = "v=STSv1; id=20200101T000000Z";
    let per = time(10_000, || {
        let _ = std::hint::black_box(StsRecord::parse(std::hint::black_box(txt)).unwrap());
    });
    let b = budget(1, 5);
    assert!(per < b, "StsRecord::parse {per:?} (budget {b:?})");
}

#[test]
fn parse_policy_under_budget() {
    let p = "version: STSv1\nmode: enforce\nmx: mail.example.com\nmx: backup.example.com\nmx: *.mx.example.net\nmax_age: 604800\n";
    let per = time(10_000, || {
        let _ = std::hint::black_box(Policy::parse(std::hint::black_box(p)).unwrap());
    });
    let b = budget(5, 25);
    assert!(per < b, "Policy::parse {per:?} (budget {b:?})");
}

#[test]
fn mx_matches_under_budget() {
    let per = time(10_000, || {
        let _ = std::hint::black_box(mx_matches(
            std::hint::black_box("mx1.example.com"),
            std::hint::black_box("*.example.com"),
        ));
    });
    let b = budget(1, 5);
    assert!(per < b, "mx_matches wildcard {per:?} (budget {b:?})");
}

#[test]
fn enforce_under_budget() {
    let p = Policy {
        mode: PolicyMode::Enforce,
        mx: vec![
            "mail.example.com".into(),
            "backup.example.com".into(),
            "*.mx.example.net".into(),
        ],
        max_age: 604_800,
    };
    let per = time(10_000, || {
        let _ = std::hint::black_box(enforce(
            std::hint::black_box(&p),
            std::hint::black_box("primary.mx.example.net"),
        ));
    });
    let b = budget(3, 15);
    assert!(per < b, "enforce 3-mx last-match {per:?} (budget {b:?})");
}
