//! Regression budgets for `mailrs-rfc2231`. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_rfc2231::{decode_param_value, encode_param};

const ITERS: usize = 200;

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
fn encode_ascii_under_budget() {
    let median = time_median(|| {
        let _ = encode_param("filename", "attachment.pdf");
    });
    // Budget: 5 µs (release ~25 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "encode_ascii median {median:?} > {budget:?}"
    );
}

#[test]
fn encode_japanese_under_budget() {
    let median = time_median(|| {
        let _ = encode_param("filename", "日本語ファイル.pdf");
    });
    // Budget: 10 µs (release ~140 ns).
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "encode_japanese median {median:?} > {budget:?}"
    );
}

#[test]
fn decode_quoted_under_budget() {
    let median = time_median(|| {
        let _ = decode_param_value("\"attachment.pdf\"");
    });
    // Budget: 5 µs (release ~15 ns).
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "decode_quoted median {median:?} > {budget:?}"
    );
}

#[test]
fn decode_extended_utf8_under_budget() {
    let median = time_median(|| {
        let _ = decode_param_value("UTF-8''%E6%97%A5%E6%9C%AC.pdf");
    });
    // Budget: 10 µs (release ~95 ns).
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "decode_extended median {median:?} > {budget:?}"
    );
}
