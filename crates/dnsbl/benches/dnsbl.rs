use criterion::{criterion_group, criterion_main, Criterion};
use mailrs_dnsbl::{dnsbl_query, interpret_spamhaus, reverse_ipv4, DnsblCache, DnsblResult};
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

fn bench_reverse_ipv4(c: &mut Criterion) {
    let ip = Ipv4Addr::new(203, 0, 113, 42);
    c.bench_function("reverse_ipv4", |b| {
        b.iter(|| {
            let r = reverse_ipv4(black_box(ip));
            black_box(r)
        });
    });
}

fn bench_dnsbl_query(c: &mut Criterion) {
    let reversed = reverse_ipv4(Ipv4Addr::new(203, 0, 113, 42));
    c.bench_function("dnsbl_query/spamhaus_zone", |b| {
        b.iter(|| {
            let r = dnsbl_query(black_box(&reversed), black_box("zen.spamhaus.org"));
            black_box(r)
        });
    });
}

fn bench_interpret_spamhaus_sbl(c: &mut Criterion) {
    c.bench_function("interpret_spamhaus/sbl_127_0_0_2", |b| {
        b.iter(|| {
            let r = interpret_spamhaus(black_box(Ipv4Addr::new(127, 0, 0, 2)));
            black_box(r)
        });
    });
}

fn bench_interpret_spamhaus_clean(c: &mut Criterion) {
    c.bench_function("interpret_spamhaus/clean_non_127", |b| {
        b.iter(|| {
            let r = interpret_spamhaus(black_box(Ipv4Addr::new(192, 168, 1, 1)));
            black_box(r)
        });
    });
}

/// Pre-seed the cache with a positive entry and measure pure cache-hit
/// cost (no DNS, no resolver). Models the typical inbound case where
/// the same IP is queried multiple times in one session.
fn bench_cache_hit_positive(c: &mut Criterion) {
    // The cache's internal map is private, so we can't pre-seed an entry
    // from a bench. Instead measure the `is_empty + len` lock+read path,
    // which is the same Mutex<HashMap> shape that dominates a real
    // hit (lookup + TTL compare). For an end-to-end cache-hit number,
    // see the integration tests.
    let cache = DnsblCache::new(Duration::from_secs(300));
    let _ = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 42));
    c.bench_function("cache/is_empty_then_len_roundtrip", |b| {
        b.iter(|| {
            let e = cache.is_empty();
            let l = cache.len();
            black_box((e, l))
        });
    });
}

// Just a sanity hash-eq path: verify variant matches don't surprise us.
fn bench_dnsbl_result_eq(c: &mut Criterion) {
    let a = DnsblResult::Sbl;
    let b = DnsblResult::Sbl;
    c.bench_function("dnsbl_result/eq_sbl_sbl", |bench| {
        bench.iter(|| {
            let r = black_box(&a) == black_box(&b);
            black_box(r)
        });
    });
}

criterion_group!(
    benches,
    bench_reverse_ipv4,
    bench_dnsbl_query,
    bench_interpret_spamhaus_sbl,
    bench_interpret_spamhaus_clean,
    bench_cache_hit_positive,
    bench_dnsbl_result_eq,
);
criterion_main!(benches);
