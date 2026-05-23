//! Regression-catch budgets. See [`BUDGETS.md`](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_rfc2047::decode;

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
fn ascii_passthrough_under_budget() {
    let input = b"This is an ASCII subject, plain English with no encoding.";
    let median = time_median(|| {
        let _ = decode(input);
    });
    // Budget: 1 µs (release P95 ~25 ns). The ASCII fast-path scans
    // for "=?" and returns borrowed if absent — bounded by input
    // length, no allocation.
    let budget = Duration::from_micros(1);
    assert!(
        median < budget,
        "ascii_passthrough median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn utf8_base64_under_budget() {
    let input = b"=?UTF-8?B?VGVzdCBNZXNzYWdl?=";
    let median = time_median(|| {
        let _ = decode(input);
    });
    // Budget: 2 µs (release P95 ~66 ns).
    let budget = Duration::from_micros(2);
    assert!(
        median < budget,
        "utf8_base64 median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn iso_2022_jp_under_budget() {
    let input = b"=?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?=";
    let median = time_median(|| {
        let _ = decode(input);
    });
    // Budget: 5 µs (release P95 ~154 ns). ISO-2022-JP via encoding_rs
    // is the heaviest decode path for typical Japanese subjects.
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "iso_2022_jp median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn mixed_ascii_and_encoded_under_budget() {
    let input = b"Re: =?UTF-8?B?VGVzdA==?= regarding the proposal";
    let median = time_median(|| {
        let _ = decode(input);
    });
    // Budget: 5 µs (release P95 ~104 ns, but CI/loaded-machine variance
    // can push it past 2 µs intermittently). Wider headroom catches
    // order-of-magnitude regressions without false-failing on background
    // CPU contention.
    let budget = Duration::from_micros(5);
    assert!(
        median < budget,
        "mixed_ascii_and_encoded median {median:?} exceeded {budget:?}"
    );
}
