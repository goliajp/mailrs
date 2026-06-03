//! sieve-core perf bench — v4 ckpt 25.
//!
//! Three things got rewritten here:
//!   * `match_str::match_string` is now zero-alloc (was lowering both
//!     sides into fresh Strings per call).
//!   * `match_str::glob_match` now splits the pattern on `*` and
//!     drives each literal chunk with memchr2-anchored substring
//!     search (was a recursive byte-by-byte backtracker).
//!   * The lexer's `#` line-comment and `/* ... */` block-comment
//!     skip loops use memchr now.
//!
//! Hot path it covers: every `header` / `address` / `envelope` /
//! `hasflag` test the server runs against an inbound message.

use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_sieve_core::{eval_script, parse_script, tokenize};
use std::hint::black_box;

const TYPICAL_SCRIPT: &str = r#"
# typical inbox filter — flag spam, file newsletters
require ["fileinto", "imap4flags"];

if header :contains "Subject" "SALE" {
    addflag "\\Flagged";
    fileinto "Junk";
    stop;
}

if address :is "From" "newsletter@example.com" {
    fileinto "Newsletters";
    stop;
}

if header :matches "List-Id" "<*.lists.example.com>" {
    fileinto "Lists";
}

keep;
"#;

const HEAVY_SCRIPT: &str = r#"
# more rules, more headers, more comments — exercises the
# match_string / glob_match path harder.
require ["fileinto", "imap4flags", "envelope"];

# block comments shouldn't slow us down /* xxx */ /* yyy */
if header :contains ["Subject", "From", "To"]
       ["urgent", "free", "winner", "lottery"] {
    addflag "\\Flagged";
    fileinto "Junk";
    stop;
}

if address :matches "From" "*@*.spam.example" {
    fileinto "Junk";
    stop;
}

if envelope :is "from" "trusted@example.org" {
    fileinto "INBOX";
    stop;
}

if header :matches "List-Id" "<*.lists.*.com>" {
    fileinto "Lists";
}

keep;
"#;

const MESSAGE: &[u8] = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: SALE today only!\r\nList-Id: <weekly.lists.example.com>\r\n\r\nbody here\r\n";

fn bench_tokenize(c: &mut Criterion) {
    let mut group = c.benchmark_group("sieve_core/tokenize");
    group.bench_function("typical", |b| {
        b.iter(|| {
            let _ = tokenize(black_box(TYPICAL_SCRIPT));
        });
    });
    group.bench_function("heavy", |b| {
        b.iter(|| {
            let _ = tokenize(black_box(HEAVY_SCRIPT));
        });
    });
    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("sieve_core/compile");
    group.bench_function("typical", |b| {
        b.iter(|| {
            let _ = parse_script(black_box(TYPICAL_SCRIPT));
        });
    });
    group.bench_function("heavy", |b| {
        b.iter(|| {
            let _ = parse_script(black_box(HEAVY_SCRIPT));
        });
    });
    group.finish();
}

fn bench_evaluate(c: &mut Criterion) {
    let mut group = c.benchmark_group("sieve_core/evaluate");
    group.bench_function("typical", |b| {
        b.iter(|| {
            let _ = eval_script(black_box(TYPICAL_SCRIPT), black_box(MESSAGE));
        });
    });
    group.bench_function("heavy", |b| {
        b.iter(|| {
            let _ = eval_script(black_box(HEAVY_SCRIPT), black_box(MESSAGE));
        });
    });
    group.finish();
}

criterion_group!(benches, bench_tokenize, bench_compile, bench_evaluate);
criterion_main!(benches);
