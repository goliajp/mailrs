//! Regression budgets for `mailrs-sieve`. See BUDGETS.md.

use std::time::{Duration, Instant};
use mailrs_sieve::{compile_sieve, evaluate_sieve};

const ITERS: usize = 100;
const SCRIPT: &str = "require \"fileinto\";\nkeep;";
const MSG: &[u8] = b"From: a@example.com\r\n\r\nbody";

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

#[test]
fn compile_sieve_under_budget() {
    let median = time_median(|| {
        let _ = compile_sieve(SCRIPT);
    });
    // Budget: 200 µs — sieve-rs allocates a parse tree.
    assert!(
        median < Duration::from_micros(200),
        "compile_sieve median {median:?} exceeds 200µs"
    );
}

#[test]
fn evaluate_sieve_under_budget() {
    let compiled = compile_sieve(SCRIPT).expect("compile");
    let median = time_median(|| {
        let _ = evaluate_sieve(&compiled, MSG);
    });
    assert!(
        median < Duration::from_micros(100),
        "evaluate_sieve median {median:?} exceeds 100µs"
    );
}
