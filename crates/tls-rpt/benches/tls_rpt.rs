//! Parser + builder microbenchmarks for mailrs-tls-rpt.

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_tls_rpt::{
    FailureEvent, FailureType, PolicyType, ReportBuilder, SuccessEvent, TlsRptRecord,
};
use std::hint::black_box;

const RECORD_SINGLE: &str = "v=TLSRPTv1; rua=mailto:tlsrpt@example.com";
const RECORD_MULTI: &str =
    "v=TLSRPTv1; rua=mailto:tlsrpt@example.com,https://reports.example.com/v1/tlsrpt,mailto:backup-tlsrpt@example.com";

fn bench_record_parse(c: &mut Criterion) {
    c.bench_function("parse/record_single", |b| {
        b.iter(|| black_box(TlsRptRecord::parse(black_box(RECORD_SINGLE)).unwrap()));
    });
    c.bench_function("parse/record_multi", |b| {
        b.iter(|| black_box(TlsRptRecord::parse(black_box(RECORD_MULTI)).unwrap()));
    });
}

fn bench_report_build(c: &mut Criterion) {
    c.bench_function("report/build_100_success", |b| {
        b.iter(|| {
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
            black_box(builder.build().unwrap());
        });
    });
    c.bench_function("report/build_mixed_100", |b| {
        b.iter(|| {
            let mut builder = ReportBuilder::new()
                .organization_name("Test")
                .contact_info("mailto:x@y")
                .report_id("r")
                .date_range("a", "b");
            for i in 0..50 {
                builder.record_success(SuccessEvent {
                    policy_domain: format!("d{}.example", i % 10),
                    policy_type: PolicyType::Sts,
                    mx_host: format!("mx{}.example", i % 3),
                });
                builder.record_failure(FailureEvent {
                    policy_domain: format!("d{}.example", i % 10),
                    policy_type: PolicyType::Sts,
                    mx_host: Some(format!("mx{}.example", i % 3)),
                    result_type: FailureType::CertificateExpired,
                    sending_mta_ip: None,
                    receiving_ip: None,
                    receiving_mx_helo: None,
                    additional_information: None,
                    failure_reason_code: None,
                });
            }
            black_box(builder.build().unwrap());
        });
    });
}

fn bench_report_serialize(c: &mut Criterion) {
    let mut builder = ReportBuilder::new()
        .organization_name("Test")
        .contact_info("mailto:x@y")
        .report_id("r")
        .date_range("2026-05-23T00:00:00Z", "2026-05-24T00:00:00Z");
    for i in 0..100 {
        builder.record_success(SuccessEvent {
            policy_domain: format!("d{}.example", i % 5),
            policy_type: PolicyType::Sts,
            mx_host: "mx.example".into(),
        });
    }
    let report = builder.build().unwrap();
    c.bench_function("report/serialize_json", |b| {
        b.iter(|| {
            let _ = black_box(serde_json::to_vec(black_box(&report)).unwrap());
        });
    });
}

criterion_group!(
    benches,
    bench_record_parse,
    bench_report_build,
    bench_report_serialize,
);
criterion_main!(benches);
