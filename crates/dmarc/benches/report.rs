//! Microbenchmarks for mailrs-dmarc pure helpers.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_dmarc::{
    DmarcResultRecord, extract_rua_from_dmarc_record, format_report_email,
    generate_dmarc_report_xml,
};

fn sample_results(n: usize) -> Vec<DmarcResultRecord> {
    (0..n)
        .map(|i| DmarcResultRecord {
            source_ip: format!("192.0.2.{}", i % 255),
            from_domain: format!("sender{}.example.com", i % 10),
            spf_result: if i % 3 == 0 {
                "fail".into()
            } else {
                "pass".into()
            },
            dkim_result: if i % 4 == 0 {
                "fail".into()
            } else {
                "pass".into()
            },
            dmarc_result: "pass".into(),
            disposition: "none".into(),
        })
        .collect()
}

fn bench_generate_xml(c: &mut Criterion) {
    let mut group = c.benchmark_group("generate_xml");
    let small = sample_results(10);
    group.bench_function("n10", |b| {
        b.iter(|| {
            generate_dmarc_report_xml(
                black_box("Org"),
                black_box("a@x"),
                black_box("r"),
                black_box("example.com"),
                black_box(0),
                black_box(86400),
                black_box(&small),
            )
        })
    });
    let mid = sample_results(500);
    group.bench_function("n500", |b| {
        b.iter(|| {
            generate_dmarc_report_xml(
                black_box("Org"),
                black_box("a@x"),
                black_box("r"),
                black_box("example.com"),
                black_box(0),
                black_box(86400),
                black_box(&mid),
            )
        })
    });
    group.finish();
}

fn bench_format_email(c: &mut Criterion) {
    let xml = generate_dmarc_report_xml("Org", "a@x", "r", "x.com", 0, 86400, &sample_results(50));
    c.bench_function("format_report_email", |b| {
        b.iter(|| {
            format_report_email(
                black_box("a@x"),
                black_box("rua@y"),
                black_box("x.com"),
                black_box("r"),
                black_box("2026-05-20"),
                black_box(&xml),
            )
        })
    });
}

fn bench_extract_rua(c: &mut Criterion) {
    let typical = "v=DMARC1; p=quarantine; rua=mailto:dmarc@example.com; ruf=mailto:forensic@example.com; fo=1; pct=100; adkim=r; aspf=r";
    c.bench_function("extract_rua_typical", |b| {
        b.iter(|| extract_rua_from_dmarc_record(black_box(typical)))
    });
}

criterion_group!(
    benches,
    bench_generate_xml,
    bench_format_email,
    bench_extract_rua
);
criterion_main!(benches);
