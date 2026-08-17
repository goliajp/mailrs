//! Perf regression gates for the mailrs-tls-rpt parsers + builder.
//!
//! Budgets are ~10× release P95 so we catch order-of-magnitude
//! regressions without flaking under load.
//!
//! **Release only**, joining the 23 files already gated that way. The
//! 5× debug slack these carried was not enough at the one moment that
//! matters: the 2026-08-17 deploy gate red on
//! `parse single rua 10.362µs (budget 10µs)` — 3.6% over — and
//! `build 100-success 174.986µs (budget 150µs)`, while the same four
//! passed in 0.06 s alone and `perf-gates.sh` (release) passed in the
//! same run. `cargo test --workspace` runs 231 test binaries at once,
//! and a debug budget under that measures the host, which is what
//! `.claude/CLAUDE.md` says about asserting an optimised-build number
//! in a dev build.
//!
//! The other 16 debug-mode gates are left alone: they survived two
//! full-load runs the same day, so their authors' slack is holding.
//! This is the one with evidence against it.
#![cfg(not(debug_assertions))]

use std::time::{Duration, Instant};

use mailrs_tls_rpt::{PolicyType, ReportBuilder, SuccessEvent, TlsRptRecord};

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

/// The release budget. The second argument is the debug one this file
/// used to fall back to; it is kept at the call sites as a record of
/// what was measured, and no longer consulted — nothing here runs in a
/// debug build.
fn budget(release_us: u64, _debug_us: u64) -> Duration {
    Duration::from_micros(release_us)
}

#[test]
fn parse_record_single_under_budget() {
    let r = "v=TLSRPTv1; rua=mailto:tlsrpt@example.com";
    let per = time(10_000, || {
        let _ = std::hint::black_box(TlsRptRecord::parse(std::hint::black_box(r)).unwrap());
    });
    let b = budget(2, 10);
    assert!(
        per < b,
        "TlsRptRecord::parse single rua {per:?} (budget {b:?})"
    );
}

#[test]
fn parse_record_multi_under_budget() {
    let r = "v=TLSRPTv1; rua=mailto:tlsrpt@example.com,https://reports.example.com/v1/tlsrpt,mailto:backup@example.com";
    let per = time(10_000, || {
        let _ = std::hint::black_box(TlsRptRecord::parse(std::hint::black_box(r)).unwrap());
    });
    let b = budget(3, 15);
    assert!(per < b, "TlsRptRecord::parse 3-rua {per:?} (budget {b:?})");
}

#[test]
fn build_100_success_under_budget() {
    let per = time(1_000, || {
        let mut builder = ReportBuilder::new()
            .organization_name("Test")
            .contact_info("mailto:x@y")
            .report_id("r")
            .date_range("a", "b");
        for _ in 0..100 {
            builder.record_success(SuccessEvent {
                policy_domain: "example.com".into(),
                policy_type: PolicyType::Sts,
                mx_host: "mail.example.com".into(),
            });
        }
        let _ = std::hint::black_box(builder.build().unwrap());
    });
    let b = budget(30, 150);
    assert!(
        per < b,
        "ReportBuilder::build 100-success {per:?} (budget {b:?})"
    );
}

#[test]
fn serialize_100_success_under_budget() {
    let mut builder = ReportBuilder::new()
        .organization_name("Test")
        .contact_info("mailto:x@y")
        .report_id("r")
        .date_range("2026-05-23T00:00:00Z", "2026-05-24T00:00:00Z");
    for _ in 0..100 {
        builder.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(),
        });
    }
    let report = builder.build().unwrap();
    let per = time(1_000, || {
        let _ = std::hint::black_box(serde_json::to_vec(std::hint::black_box(&report)).unwrap());
    });
    let b = budget(8, 40);
    assert!(
        per < b,
        "serde_json::to_vec 100-success {per:?} (budget {b:?})"
    );
}
