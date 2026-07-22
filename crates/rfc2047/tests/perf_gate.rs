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

/// Budget for the profile the test is actually running under.
///
/// These budgets were derived from release P95s, but `cargo test`
/// builds in dev — where the same decode runs one to two orders of
/// magnitude slower, so a release-derived number leaves no headroom and
/// fails whenever the machine is busy. Two of the budgets in this file
/// had already been widened for that reason; splitting by profile
/// applies it uniformly without touching the release numbers, which
/// stay exactly as measured.
///
/// The dev budget is not noise tolerance for its own sake — it still
/// catches the order-of-magnitude regressions worth catching, which is
/// all a dev-profile timing can honestly assert.
fn budget(release_us: u64, dev_us: u64) -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_micros(dev_us)
    } else {
        Duration::from_micros(release_us)
    }
}

#[test]
fn ascii_passthrough_under_budget() {
    let input = b"This is an ASCII subject, plain English with no encoding.";
    let median = time_median(|| {
        let _ = decode(input);
    });
    // Release 1 µs (P95 ~25 ns). The ASCII fast-path scans for "=?"
    // and returns borrowed if absent — bounded by input length, no
    // allocation.
    let budget = budget(1, 20);
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
    // Release 2 µs (P95 ~66 ns). Measured 5.5-5.8 µs in dev under
    // workspace test load — this is the budget that kept false-failing
    // before the profile split.
    let budget = budget(2, 20);
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
    // Release 20 µs (P95 ~154 ns). ISO-2022-JP via encoding_rs is the
    // heaviest decode path for typical Japanese subjects; dev profile
    // under workspace test load borders 5-8 µs.
    let budget = budget(20, 40);
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
    // Release 5 µs (P95 ~104 ns). Measured 5.6 µs in dev under load —
    // the previous 5 µs was already borderline in dev, which is what
    // the profile split fixes.
    let budget = budget(5, 20);
    assert!(
        median < budget,
        "mixed_ascii_and_encoded median {median:?} exceeded {budget:?}"
    );
}
