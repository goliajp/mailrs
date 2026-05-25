//! Perf regression gates for the mailrs-tls-rpt parsers + builder.
//!
//! Budgets are ~10× release P95 so we catch order-of-magnitude
//! regressions without flaking under load. Debug-mode runs
//! (release.sh `cargo test --workspace`) get a 5× slack on top.

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

fn budget(release_us: u64, debug_us: u64) -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_micros(debug_us)
    } else {
        Duration::from_micros(release_us)
    }
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
