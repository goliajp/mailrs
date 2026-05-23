//! Head-to-head: `mailrs-spf` record parsing vs `mail-auth` 0.9 (the SPF
//! half of Stalwart's email-auth crate, which is the de-facto Rust
//! competitor and the crate this whole stone was carved out to replace —
//! see DEPS_AUDIT #1).
//!
//! Both libraries parse a wire-format `v=spf1 …` TXT record into a structured
//! representation. We compare:
//!
//! * `simple` — `v=spf1 ip4:203.0.113.0/24 -all` (3 directives + version).
//! * `complex` — 8-mechanism record (the same input the in-crate bench
//!   labels as `parse/complex_record_8_mechanisms`).
//! * `pathological` — repeated includes; not realistic but pressures
//!   alloc paths.
//!
//! Honest disclosure of method: same CPU, same compiler, same release
//! profile, identical input bytes, same return-value side-effect via
//! `black_box`. Each side calls only the parse step (no DNS, no async
//! runtime) so the comparison is apples-to-apples on what the library
//! itself does. Numbers are recorded in `BENCHMARKS.md` and the workspace
//! `PERFORMANCE.md`.

use criterion::{Criterion, criterion_group, criterion_main};
use mail_auth::common::parse::TxtRecordParser;
use mail_auth::spf::Spf as MailAuthSpf;
use mailrs_spf::Record;
use std::hint::black_box;

const SIMPLE: &str = "v=spf1 ip4:203.0.113.0/24 -all";
const COMPLEX: &str = "v=spf1 ip4:203.0.113.0/24 ip4:198.51.100.0/24 \
    ip6:2001:db8::/32 a:mail.example.com mx:example.com \
    include:_spf.google.com include:spf.protection.outlook.com -all";
const PATHOLOGICAL: &str = "v=spf1 include:a.example include:b.example \
    include:c.example include:d.example include:e.example include:f.example \
    include:g.example include:h.example -all";

fn bench_parse_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/simple");
    group.bench_function("mailrs_spf", |b| {
        b.iter(|| {
            let r = Record::parse(black_box(SIMPLE));
            black_box(r.unwrap())
        });
    });
    group.bench_function("mail_auth", |b| {
        b.iter(|| {
            let r = MailAuthSpf::parse(black_box(SIMPLE.as_bytes()));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

fn bench_parse_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/complex_8_mechanisms");
    group.bench_function("mailrs_spf", |b| {
        b.iter(|| {
            let r = Record::parse(black_box(COMPLEX));
            black_box(r.unwrap())
        });
    });
    group.bench_function("mail_auth", |b| {
        b.iter(|| {
            let r = MailAuthSpf::parse(black_box(COMPLEX.as_bytes()));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

fn bench_parse_pathological(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/pathological_8_includes");
    group.bench_function("mailrs_spf", |b| {
        b.iter(|| {
            let r = Record::parse(black_box(PATHOLOGICAL));
            black_box(r.unwrap())
        });
    });
    group.bench_function("mail_auth", |b| {
        b.iter(|| {
            let r = MailAuthSpf::parse(black_box(PATHOLOGICAL.as_bytes()));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_simple,
    bench_parse_complex,
    bench_parse_pathological,
);
criterion_main!(benches);
