//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_smtp_client::connection::dot_stuff;
use mailrs_smtp_client::mx::{MxRecord, sort_mx_records};
use mailrs_smtp_client::response::parse_response;

const ITERS: usize = 100;

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

const LONG_EHLO: &str = "\
250-smtp.example.com Hello [192.0.2.1]\r\n\
250-SIZE 36700160\r\n\
250-STARTTLS\r\n\
250-8BITMIME\r\n\
250-PIPELINING\r\n\
250-AUTH PLAIN LOGIN\r\n\
250-CHUNKING\r\n\
250-DSN\r\n\
250-SMTPUTF8\r\n\
250 ENHANCEDSTATUSCODES\r\n";

#[test]
fn parse_response_long_ehlo_under_budget() {
    let median = time_median(|| {
        let _ = parse_response(LONG_EHLO);
    });
    // Budget: 50 µs. Observed P95: ~1 µs (parsing 10 lines).
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "parse_response(LONG_EHLO) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn dot_stuff_body_with_dots_under_budget() {
    let body_with_dots = format!(
        "From: a@x\r\nTo: b@x\r\nSubject: dots\r\n\r\n{}",
        ".dot at start of every other line\r\nnormal line\r\n".repeat(50)
    );
    let median = time_median(|| {
        let _ = dot_stuff(body_with_dots.as_bytes());
    });
    // Budget: 500 µs. Observed P95: ~20 µs (4 KB body w/ dots every other line).
    let budget = Duration::from_micros(500);
    assert!(
        median < budget,
        "dot_stuff(with dots) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn sort_mx_records_n20_under_budget() {
    let mut records: Vec<MxRecord> = (0..20)
        .map(|i| MxRecord {
            exchange: format!("mx{i}.example.com"),
            priority: (i * 7 + 13) % 100,
        })
        .collect();
    let median = time_median(|| {
        sort_mx_records(&mut records);
    });
    // Budget: 20 µs. Observed P95: ~500 ns.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "sort_mx_records(n=20) median {median:?} exceeded {budget:?}"
    );
}
