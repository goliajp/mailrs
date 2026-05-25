//! Regression budgets for `mailrs-imap-codec`. See BUDGETS.md.

use std::time::{Duration, Instant};
use bytes::BytesMut;
use mailrs_imap_codec::ImapCodec;
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
    // Budget: 10 µs (release ~200 ns).
    assert!(
        median < Duration::from_micros(10),
        "ImapCodec::decode median {median:?} exceeds 10µs"
    );
}
