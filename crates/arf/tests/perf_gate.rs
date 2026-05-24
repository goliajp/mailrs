//! Regression budgets for `mailrs-arf`. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_arf::parse;

const ITERS: usize = 200;

const HOTMAIL_FBL_SAMPLE: &[u8] = b"From: staff@hotmail.com\r\n\
Subject: complaint\r\n\
Content-Type: multipart/report; report-type=feedback-report\r\n\
\r\n\
--b\r\n\
Content-Type: message/feedback-report\r\n\
\r\n\
Feedback-Type: abuse\r\n\
User-Agent: Hotmail FBL\r\n\
Version: 1\r\n\
Original-Mail-From: <bulk@example.com>\r\n\
Original-Rcpt-To: <victim@hotmail.com>\r\n\
Source-IP: 192.0.2.42\r\n\
Reported-Domain: example.com\r\n";

const NOT_ARF: &[u8] = b"From: a@b.com\r\nSubject: hi\r\n\r\nbody";

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
fn parse_hotmail_under_budget() {
    let median = time_median(|| {
        let _ = parse(HOTMAIL_FBL_SAMPLE);
    });
    // Budget: 30 µs (release ~2 µs; dev mode ~10 µs).
    assert!(
        median < Duration::from_micros(30),
        "parse Hotmail sample median {}µs exceeds 30µs",
        median.as_micros()
    );
}

#[test]
fn parse_early_exit_under_budget() {
    let median = time_median(|| {
        let _ = parse(NOT_ARF);
    });
    // Budget: 5 µs (release < 200 ns; dev mode ~1 µs).
    assert!(
        median < Duration::from_micros(5),
        "parse non-ARF early-exit median {}µs exceeds 5µs",
        median.as_micros()
    );
}
