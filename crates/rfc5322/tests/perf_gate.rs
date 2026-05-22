//! Regression-catch budgets for hot paths. See [`BUDGETS.md`](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_rfc5322::Message;

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

fn sample_message(body_kb: usize) -> Vec<u8> {
    let mut m = Vec::with_capacity(1024 + body_kb * 1024);
    m.extend_from_slice(
        b"From: alice@example.com\r\n\
          To: bob@example.com\r\n\
          Subject: hello\r\n\
          Date: Sun, 22 May 2026 09:00:00 +0900\r\n\
          Message-ID: <abc-123@example.com>\r\n\r\n",
    );
    for _ in 0..(body_kb * 1024 / 80) {
        m.extend_from_slice(
            b"This is a typical inbound message body line, ASCII text only.\r\n",
        );
    }
    m
}

#[test]
fn header_lookup_under_budget() {
    let bytes = sample_message(5);
    let median = time_median(|| {
        let m = Message::new(&bytes);
        let _ = m.header("Subject");
        let _ = m.header("From");
    });
    // Budget: 10 µs (release P95 ~280 ns; dev ~2-5 µs). Generous to
    // tolerate noisy CI; the real-perf claim lives in PERFORMANCE.md
    // backed by `cargo bench`.
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "header_lookup median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn body_locate_under_budget() {
    let bytes = sample_message(20);
    let median = time_median(|| {
        let m = Message::new(&bytes);
        let _ = m.body();
    });
    // Budget: 10 µs (release P95 ~250 ns; constant regardless of body
    // size because scanner stops at empty line).
    let budget = Duration::from_micros(10);
    assert!(
        median < budget,
        "body_locate median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn received_chain_walk_under_budget() {
    let mut bytes = Vec::with_capacity(2048);
    bytes.extend_from_slice(
        b"Received: from a.example.com\r\n\
          Received: from b.example.com\r\n\
          Received: from c.example.com\r\n\
          From: x\r\nSubject: y\r\n\r\nbody",
    );
    let median = time_median(|| {
        let m = Message::new(&bytes);
        let count = m.header_all("Received").count();
        assert_eq!(count, 3);
    });
    // Budget: 20 µs (release P95 ~340 ns; walks 3 entries).
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "received_chain_walk median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn body_cached_call_under_budget() {
    let bytes = sample_message(20);
    let m = Message::new(&bytes);
    // First call primes the cache.
    let _ = m.body();
    // Subsequent calls are O(1) — memo'd.
    let median = time_median(|| {
        let _ = m.body();
    });
    // Budget: 1 µs (release P95 ~5 ns; just a Cell::get).
    let budget = Duration::from_micros(1);
    assert!(
        median < budget,
        "body_cached median {median:?} exceeded {budget:?}"
    );
}
