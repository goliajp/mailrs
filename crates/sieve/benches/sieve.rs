use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_sieve::{compile_sieve, evaluate_sieve};
use std::hint::black_box;

const SCRIPT: &str =
    "require \"fileinto\";\nif header :is \"X-Spam\" \"YES\" { fileinto \"Junk\"; } else { keep; }";
const MSG: &[u8] = b"From: a@example.com\r\nX-Spam: NO\r\n\r\nbody";

fn bench_compile(c: &mut Criterion) {
    c.bench_function("compile_sieve/typical", |b| {
        b.iter(|| {
            let _ = compile_sieve(black_box(SCRIPT));
        });
    });
}

fn bench_evaluate(c: &mut Criterion) {
    let compiled = compile_sieve(SCRIPT).expect("compile");
    c.bench_function("evaluate_sieve/typical", |b| {
        b.iter(|| {
            let _ = evaluate_sieve(black_box(&compiled), black_box(MSG));
        });
    });
}

criterion_group!(benches, bench_compile, bench_evaluate);
criterion_main!(benches);
