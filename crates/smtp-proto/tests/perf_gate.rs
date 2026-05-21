//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_smtp_proto::address::{is_valid, split_address};
use mailrs_smtp_proto::parse::parse_command;
use mailrs_smtp_proto::response::format_ehlo_response;

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

#[test]
fn parse_command_mail_from_under_budget() {
    let median = time_median(|| {
        let _ = parse_command("MAIL FROM:<alice@example.com> SIZE=12345\r\n");
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "parse_command(MAIL FROM) median {median:?} exceeded {budget:?}");
}

#[test]
fn parse_command_auth_plain_under_budget() {
    let median = time_median(|| {
        let _ = parse_command("AUTH PLAIN AGFsaWNlAHBhc3N3b3Jk\r\n");
    });
    // Budget: 20 µs. Observed P95: ~200 ns.
    let budget = Duration::from_micros(20);
    assert!(median < budget, "parse_command(AUTH PLAIN) median {median:?} exceeded {budget:?}");
}

#[test]
fn address_is_valid_under_budget() {
    let median = time_median(|| {
        let _ = is_valid("alice.smith+work@example.co.jp");
    });
    // Budget: 10 µs. Observed P95: ~100 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "is_valid median {median:?} exceeded {budget:?}");
}

#[test]
fn address_split_under_budget() {
    let median = time_median(|| {
        let _ = split_address("alice.smith+work@example.co.jp");
    });
    // Budget: 10 µs. Observed P95: ~100 ns.
    let budget = Duration::from_micros(10);
    assert!(median < budget, "split_address median {median:?} exceeded {budget:?}");
}

#[test]
fn format_ehlo_response_under_budget() {
    let caps = ["SIZE 36700160", "STARTTLS", "8BITMIME", "PIPELINING", "AUTH PLAIN LOGIN", "DSN"];
    let median = time_median(|| {
        let _ = format_ehlo_response("mail.example.com", &caps);
    });
    // Budget: 50 µs. Observed P95: ~500 ns.
    let budget = Duration::from_micros(50);
    assert!(median < budget, "format_ehlo_response median {median:?} exceeded {budget:?}");
}
