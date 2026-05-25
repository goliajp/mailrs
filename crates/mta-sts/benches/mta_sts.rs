//! Parser + enforce-fn microbenchmarks for mailrs-mta-sts.

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_mta_sts::{Policy, PolicyMode, StsRecord, enforce, mx_matches};
use std::hint::black_box;

const TXT: &str = "v=STSv1; id=20200101T000000Z";

const POLICY: &str = "\
version: STSv1
mode: enforce
mx: mail.example.com
mx: backup.example.com
mx: *.mx.example.net
max_age: 604800
";

fn bench_record_parse(c: &mut Criterion) {
    c.bench_function("parse/sts_record", |b| {
        b.iter(|| black_box(StsRecord::parse(black_box(TXT)).unwrap()));
    });
}

fn bench_policy_parse(c: &mut Criterion) {
    c.bench_function("parse/policy", |b| {
        b.iter(|| black_box(Policy::parse(black_box(POLICY)).unwrap()));
    });
}

fn bench_mx_matches(c: &mut Criterion) {
    c.bench_function("mx_matches/literal", |b| {
        b.iter(|| {
            black_box(mx_matches(
                black_box("mail.example.com"),
                black_box("mail.example.com"),
            ))
        });
    });
    c.bench_function("mx_matches/wildcard_match", |b| {
        b.iter(|| {
            black_box(mx_matches(
                black_box("mx1.example.com"),
                black_box("*.example.com"),
            ))
        });
    });
    c.bench_function("mx_matches/wildcard_no_match", |b| {
        b.iter(|| {
            black_box(mx_matches(
                black_box("a.b.example.com"),
                black_box("*.example.com"),
            ))
        });
    });
}

fn bench_enforce(c: &mut Criterion) {
    let p = Policy {
        mode: PolicyMode::Enforce,
        mx: vec![
            "mail.example.com".into(),
            "backup.example.com".into(),
            "*.mx.example.net".into(),
        ],
        max_age: 604800,
    };
    c.bench_function("enforce/3_mx_first_match", |b| {
        b.iter(|| black_box(enforce(black_box(&p), black_box("mail.example.com"))));
    });
    c.bench_function("enforce/3_mx_last_match", |b| {
        b.iter(|| black_box(enforce(black_box(&p), black_box("primary.mx.example.net"))));
    });
    c.bench_function("enforce/3_mx_no_match_deny", |b| {
        b.iter(|| black_box(enforce(black_box(&p), black_box("attacker.com"))));
    });
}

criterion_group!(
    benches,
    bench_record_parse,
    bench_policy_parse,
    bench_mx_matches,
    bench_enforce,
);
criterion_main!(benches);
