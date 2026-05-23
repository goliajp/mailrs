//! Regression budgets for `mailrs-mime`. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_mime::parse;

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

const SIMPLE: &[u8] = b"Content-Type: text/plain\r\n\r\nhello";

const MULTIPART: &[u8] = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
hello\r\n\
--x\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>hello</p>\r\n\
--x--\r\n";

const INVITE: &[u8] = b"Content-Type: multipart/alternative; boundary=\"x\"\r\n\
\r\n\
--x\r\n\
Content-Type: text/plain\r\n\
\r\n\
Meeting invitation\r\n\
--x\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n\
--x--\r\n";

#[test]
fn parse_simple_under_budget() {
    let median = time_median(|| {
        let _ = parse(SIMPLE);
    });
    // Budget: 5 µs (release ~170 ns).
    let budget = Duration::from_micros(5);
    assert!(median < budget, "parse_simple {median:?} > {budget:?}");
}

#[test]
fn parse_multipart_under_budget() {
    let median = time_median(|| {
        let _ = parse(MULTIPART);
    });
    // Budget: 10 µs (release ~830 ns).
    let budget = Duration::from_micros(10);
    assert!(median < budget, "parse_multipart {median:?} > {budget:?}");
}

#[test]
fn find_text_calendar_under_budget() {
    let median = time_median(|| {
        let p = parse(INVITE);
        let _ = p.find_by_content_type("text/calendar");
    });
    // Budget: 20 µs (release ~1.4 µs).
    let budget = Duration::from_micros(20);
    assert!(median < budget, "find_text_calendar {median:?} > {budget:?}");
}
