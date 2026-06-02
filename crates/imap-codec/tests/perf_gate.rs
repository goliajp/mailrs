//! Regression budgets for `mailrs-imap-codec`. See BUDGETS.md.

use bytes::BytesMut;
use mailrs_imap_codec::ImapCodec;
use std::time::{Duration, Instant};
use tokio_util::codec::Decoder;

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
fn decode_login_under_budget() {
    let median = time_median(|| {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from(&b"a001 LOGIN user pass\r\n"[..]);
        let _ = codec.decode(&mut buf);
    });
    // Budget: 10 µs (release ~72 ns; ~140× headroom).
    assert!(
        median < Duration::from_micros(10),
        "ImapCodec::decode median {median:?} exceeds 10µs"
    );
}

#[test]
fn decode_literal_100k_under_budget() {
    // 100 KB literal-mode decode — the APPEND / FETCH BODY[…] hot
    // path for large attachments. Per v4 round 1 measurements,
    // release ≈ 13.2 µs (memcpy-bound). Budget at 100 µs gives
    // ~7.5× headroom: catches an algorithmic regression (e.g.
    // accidental double-copy of the payload) but not thermal /
    // parallel-test noise. If this flakes under cargo-test
    // workspace parallelism, loosen the budget rather than dilute
    // the meaning of "regression".
    let mut payload = vec![b'x'; 102_400];
    payload.extend_from_slice(b"\r\n");
    let median = time_median(|| {
        let mut codec = ImapCodec::new();
        codec.expect_literal(102_400);
        let mut buf = BytesMut::from(payload.as_slice());
        let _ = codec.decode(&mut buf);
    });
    assert!(
        median < Duration::from_micros(100),
        "ImapCodec::decode literal/100k median {median:?} exceeds 100µs"
    );
}
