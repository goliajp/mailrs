//! What reading a message through the worker costs on the two hot paths:
//! the receiver's scan during DATA (attachments + Bayes tokens) and the
//! drain's search text, each against the same parse in process.
//!
//! The worker adds two pipe copies of the message and a JSON answer; on a
//! newsletter-sized message that is what these gates hold. Budgets are 3×
//! the P95 measured on studio (M-series, release); see `BUDGETS.md`.
//! Release only: the budgets belong to an optimised build.
#![cfg(not(debug_assertions))]

use std::time::{Duration, Instant};

use mailrs_parse_worker::proto::Op;
use mailrs_parse_worker::{Pool, Setup, Tokenizer, Want};

const RUNS: usize = 400;

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

fn p95(mut xs: Vec<Duration>) -> Duration {
    xs.sort();
    xs[xs.len() * 95 / 100]
}

fn measure(op: &Op, raw: &[u8]) -> (Duration, Duration) {
    let pool = Pool::new(Setup {
        bin: env!("CARGO_BIN_EXE_mailrs-parse-worker").into(),
        workers: 1,
        env: Vec::new(),
    });
    // started and warm before anything is timed
    for _ in 0..20 {
        pool.call(op, raw, Duration::from_secs(5)).unwrap();
    }
    let mut worker = Vec::with_capacity(RUNS);
    let mut in_process = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        pool.call(op, raw, Duration::from_secs(5)).unwrap();
        worker.push(t.elapsed());
        let t = Instant::now();
        std::hint::black_box(mailrs_parse_worker::answer::answer(op, raw, &mut |_| {}));
        in_process.push(t.elapsed());
    }
    (p95(worker), p95(in_process))
}

fn gate(name: &str, op: Op, total: Duration, overhead: Duration) {
    let raw = newsletter();
    let (worker, in_process) = measure(&op, &raw);
    let extra = worker.saturating_sub(in_process);
    eprintln!(
        "{name}: {} bytes, worker p95 {worker:?}, in process p95 {in_process:?}, overhead {extra:?}",
        raw.len()
    );
    assert!(
        worker < total,
        "{name}: worker p95 {worker:?} (budget {total:?})"
    );
    assert!(
        extra < overhead,
        "{name}: the worker adds {extra:?} over in process (budget {overhead:?})"
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
const BUDGET_SCAN_OVERHEAD_US: u64 = 160;
const BUDGET_BODY_US: u64 = 650;
const BUDGET_BODY_OVERHEAD_US: u64 = 150;
