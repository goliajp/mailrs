//! Head-to-head: `mailrs-dkim` vs `mail-auth` 0.9 (DKIM half).
//!
//! Comparison points (DKIM-Signature header parse only — canonicalization
//! is harder to compare apples-to-apples because `mail-auth` streams into
//! a HashContext while we return `Vec<u8>`):
//!
//! * `minimal` — bare minimum tags: v, a, d, s, h, bh, b.
//! * `realistic` — multi-header `h=`, folding whitespace, full RSA-2048-sized
//!   signature placeholder.
//!
//! Both libraries take the textual value of the DKIM-Signature header (after
//! "DKIM-Signature: " is stripped) and return a structured value.
//!
//! Method disclosure: same input bytes for both sides. mailrs-dkim's
//! `DkimHeader::parse(&str)` takes UTF-8; mail-auth's `Signature::parse(&[u8])`
//! takes bytes. We give each its native input form built from the same
//! source string, so neither side pays a conversion cost the other doesn't.

use criterion::{Criterion, criterion_group, criterion_main};
use mail_auth::dkim::Signature as MailAuthSig;
use mailrs_dkim::header::DkimHeader;
use std::hint::black_box;

const MINIMAL: &str = "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=AAAA; b=BBBB";

const REALISTIC: &str = " v=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail;\r\n\
                          \th=From:To:Subject:Date:Message-ID:MIME-Version:Content-Type;\r\n\
                          \tbh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=;\r\n\
                          \tb=signature1234567890abcdefghijklmnopqrstuvwxyz";

fn bench_parse_minimal(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/minimal");
    group.bench_function("mailrs_dkim", |b| {
        b.iter(|| {
            let r = DkimHeader::parse(black_box(MINIMAL));
            black_box(r.unwrap())
        });
    });
    group.bench_function("mail_auth", |b| {
        b.iter(|| {
            let r = MailAuthSig::parse(black_box(MINIMAL.as_bytes()));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

fn bench_parse_realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/realistic");
    group.bench_function("mailrs_dkim", |b| {
        b.iter(|| {
            let r = DkimHeader::parse(black_box(REALISTIC));
            black_box(r.unwrap())
        });
    });
    group.bench_function("mail_auth", |b| {
        b.iter(|| {
            let r = MailAuthSig::parse(black_box(REALISTIC.as_bytes()));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

criterion_group!(benches, bench_parse_minimal, bench_parse_realistic);
criterion_main!(benches);
