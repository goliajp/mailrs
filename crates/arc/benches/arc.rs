//! ARC parse + chain extract microbenchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use mailrs_arc::{ArcAuthResults, ArcChain, ArcMessageSignature, ArcSeal};

const AAR: &str =
    "i=1; spf=pass smtp.mailfrom=alice@example.com; dkim=pass header.d=example.com; dmarc=pass";
const AMS: &str = "i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; h=From:To:Subject:Date:Message-ID; bh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=; b=signature1234567890abcdefghijklmnopqrstuvwxyz";
const AS: &str = "i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; t=1700000000; b=SEAL1234567890abcdefghijklmnopqrstuvwxyz";

fn bench_aar(c: &mut Criterion) {
    c.bench_function("parse/aar", |b| {
        b.iter(|| black_box(ArcAuthResults::parse(black_box(AAR)).unwrap()));
    });
}

fn bench_ams(c: &mut Criterion) {
    c.bench_function("parse/ams", |b| {
        b.iter(|| black_box(ArcMessageSignature::parse(black_box(AMS)).unwrap()));
    });
}

fn bench_as(c: &mut Criterion) {
    c.bench_function("parse/as", |b| {
        b.iter(|| black_box(ArcSeal::parse(black_box(AS)).unwrap()));
    });
}

fn bench_chain_extract(c: &mut Criterion) {
    let two_hop = b"\
ARC-Authentication-Results: i=1; spf=pass\r\n\
ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; h=From:To:Subject; bh=BH1; b=SIG1\r\n\
ARC-Seal: i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=SEAL1\r\n\
ARC-Authentication-Results: i=2; dkim=pass\r\n\
ARC-Message-Signature: i=2; a=rsa-sha256; c=relaxed/relaxed; d=fwd.example; s=mail; h=From:To:Subject; bh=BH2; b=SIG2\r\n\
ARC-Seal: i=2; a=rsa-sha256; cv=pass; d=fwd.example; s=mail; b=SEAL2\r\n\
From: alice@example.com\r\n\r\nbody";
    c.bench_function("chain/extract_two_hop", |b| {
        b.iter(|| black_box(ArcChain::extract(black_box(two_hop)).unwrap()));
    });
}

criterion_group!(benches, bench_aar, bench_ams, bench_as, bench_chain_extract);
criterion_main!(benches);
