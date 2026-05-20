//! Microbenchmarks for shield's pure helpers (no live resolver hits).

use std::hint::black_box;
use std::net::Ipv4Addr;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_shield::dnsbl::{interpret_spamhaus, reverse_ipv4};
use mailrs_shield::greylist::{
    GreylistConfig, GreylistDecision, evaluate_triplet, triplet_key,
};
use mailrs_shield::ptr::ptr_score_from_names;

fn bench_dnsbl(c: &mut Criterion) {
    let mut group = c.benchmark_group("dnsbl");
    group.bench_function("reverse_ipv4", |b| {
        b.iter(|| reverse_ipv4(black_box(Ipv4Addr::new(1, 2, 3, 4))))
    });
    group.bench_function("interpret_spamhaus", |b| {
        b.iter(|| interpret_spamhaus(black_box(Ipv4Addr::new(127, 0, 0, 2))))
    });
    group.finish();
}

fn bench_greylist(c: &mut Criterion) {
    let cfg = GreylistConfig::default();
    let mut group = c.benchmark_group("greylist");
    group.bench_function("evaluate_first_seen", |b| {
        b.iter(|| {
            let d = evaluate_triplet(black_box(None), black_box(1000), black_box(&cfg));
            assert_eq!(d, GreylistDecision::Defer);
        })
    });
    group.bench_function("evaluate_retry", |b| {
        b.iter(|| {
            evaluate_triplet(black_box(Some(1000)), black_box(2000), black_box(&cfg))
        })
    });
    group.bench_function("triplet_key", |b| {
        b.iter(|| {
            triplet_key(
                black_box("192.0.2.1"),
                black_box("alice@example.com"),
                black_box("bob@example.org"),
            )
        })
    });
    group.finish();
}

fn bench_ptr(c: &mut Criterion) {
    let names = vec![
        "mail.example.com.".into(),
        "mx1.example.com.".into(),
        "mx2.example.com.".into(),
    ];
    c.bench_function("ptr_score_from_names_match", |b| {
        b.iter(|| ptr_score_from_names(black_box(&names), black_box("example.com")))
    });
    c.bench_function("ptr_score_from_names_no_match", |b| {
        b.iter(|| ptr_score_from_names(black_box(&names), black_box("evil.com")))
    });
}

criterion_group!(benches, bench_dnsbl, bench_greylist, bench_ptr);
criterion_main!(benches);
