//! What reading a message through the worker costs on the two hot paths:
//! the receiver's scan during DATA (attachments + Bayes tokens) and the
//! drain's search text, each against the same parse in process.
//!
//! The worker adds two pipe copies of the message and a JSON answer; on a
//! newsletter-sized message that is what these gates hold. Budgets are 3×
//! what was measured on studio (M-series, release); see `BUDGETS.md`.
//! Release only: the budgets belong to an optimised build.
#![cfg(not(debug_assertions))]

use std::time::{Duration, Instant};

use mailrs_parse_worker::proto::Op;
use mailrs_parse_worker::{Pool, Setup, Tokenizer, Want};

const RUNS: usize = 400;
const ROUNDS: usize = 3;

/// A 40 KB marketing message: text and HTML alternatives and a small
/// attachment, the shape most inbound mail has.
fn newsletter() -> Vec<u8> {
    let mut html = String::from("<html><body><table>");
    let mut text = String::new();
    for i in 0..300 {
        html.push_str(&format!(
            "<tr><td><a href=\"https://shop.example/item/{i}\">Item {i}</a></td>\
<td style=\"color:#333\">Spring sale, {i}% off this week only</td></tr>"
        ));
        text.push_str(&format!(
            "Item {i}: spring sale, {i}% off this week only\r\n"
        ));
    }
    html.push_str("</table></body></html>");
    format!(
        "From: Shop <news@shop.example>\r\nTo: u@golia.test\r\nSubject: Spring sale\r\n\
Date: Thu, 5 Nov 2026 09:00:00 +0900\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n--a\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n{html}\r\n--a--\r\n--m\r\n\
Content-Type: application/zip; name=catalog.zip\r\n\
Content-Disposition: attachment; filename=catalog.zip\r\n\
Content-Transfer-Encoding: base64\r\n\r\nUEsFBgAAAAAAAAAAAAAAAAAAAAAAAA==\r\n--m--\r\n"
    )
    .into_bytes()
}

fn percentile(mut xs: Vec<Duration>, p: usize) -> Duration {
    xs.sort();
    xs[xs.len() * p / 100]
}

fn median(mut xs: [Duration; ROUNDS]) -> Duration {
    xs.sort();
    xs[ROUNDS / 2]
}

/// One round: worker P95, in-process P95, and the median of what each
/// call through the worker took over the in-process parse timed right
/// after it.
///
/// The overhead is taken per iteration, not as the difference of the two
/// P95s: the tails of two separate distributions land on different
/// iterations, so subtracting them measures how unevenly a busy machine
/// preempted the two loops, not what the worker adds.
fn round(pool: &Pool, op: &Op, raw: &[u8]) -> (Duration, Duration, Duration) {
    let mut worker = Vec::with_capacity(RUNS);
    let mut in_process = Vec::with_capacity(RUNS);
    let mut extra = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        pool.call(op, raw, Duration::from_secs(5)).unwrap();
        let w = t.elapsed();
        let t = Instant::now();
        std::hint::black_box(mailrs_parse_worker::answer::answer(op, raw, &mut |_| {}));
        let i = t.elapsed();
        worker.push(w);
        in_process.push(i);
        extra.push(w.saturating_sub(i));
    }
    (
        percentile(worker, 95),
        percentile(in_process, 95),
        percentile(extra, 50),
    )
}

/// Each number is the median over `ROUNDS` rounds, so one round that
/// shared the machine with a burst of other work does not decide.
fn measure(op: &Op, raw: &[u8]) -> (Duration, Duration, Duration) {
    let pool = Pool::new(Setup {
        bin: env!("CARGO_BIN_EXE_mailrs-parse-worker").into(),
        workers: 1,
        env: Vec::new(),
    });
    // started and warm before anything is timed
    for _ in 0..20 {
        pool.call(op, raw, Duration::from_secs(5)).unwrap();
    }
    let rounds: [_; ROUNDS] = std::array::from_fn(|_| round(&pool, op, raw));
    (
        median(rounds.map(|r| r.0)),
        median(rounds.map(|r| r.1)),
        median(rounds.map(|r| r.2)),
    )
}

fn gate(name: &str, op: Op, total: Duration, overhead: Duration) {
    let raw = newsletter();
    let (worker, in_process, extra) = measure(&op, &raw);
    // in process far above its usual figure (BUDGETS.md) means the
    // machine was busy, not that the code got slower
    eprintln!(
        "{name}: {} bytes, worker p95 {worker:?}, in process p95 {in_process:?}, \
overhead per call (median) {extra:?}",
        raw.len()
    );
    assert!(
        worker < total,
        "{name}: worker p95 {worker:?} (budget {total:?}; in process p95 {in_process:?})"
    );
    assert!(
        extra < overhead,
        "{name}: the worker adds {extra:?} per call over in process (budget {overhead:?})"
    );
}

#[test]
fn the_receivers_scan_through_the_worker_stays_within_budget() {
    gate(
        "receiver scan",
        Op::Message(Want {
            inside: true,
            tokens: Some(Tokenizer::Raw),
            ..Want::default()
        }),
        Duration::from_micros(BUDGET_SCAN_US),
        Duration::from_micros(BUDGET_SCAN_OVERHEAD_US),
    );
}

#[test]
fn the_drains_search_text_through_the_worker_stays_within_budget() {
    gate(
        "drain body text",
        Op::Message(Want {
            body_text: true,
            ..Want::default()
        }),
        Duration::from_micros(BUDGET_BODY_US),
        Duration::from_micros(BUDGET_BODY_OVERHEAD_US),
    );
}

const BUDGET_SCAN_US: u64 = 1_700;
const BUDGET_SCAN_OVERHEAD_US: u64 = 210;
const BUDGET_BODY_US: u64 = 650;
const BUDGET_BODY_OVERHEAD_US: u64 = 170;
