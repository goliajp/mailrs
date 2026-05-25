//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_imap_proto::command::parse_command;
use mailrs_imap_proto::response::format_fetch;
use mailrs_imap_proto::sequence::{parse_sequence_set, sequence_set_to_uids};

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
fn parse_command_complex_uid_search_under_budget() {
    let cmd =
        "a004 UID SEARCH SINCE 1-Jan-2026 NOT DELETED OR FROM alice@example.com SUBJECT urgent\r\n";
    let median = time_median(|| {
        let _ = parse_command(cmd);
    });
    // Budget: 200 µs. Observed P95: ~5 µs.
    let budget = Duration::from_micros(200);
    assert!(
        median < budget,
        "parse_command(complex UID SEARCH) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn sequence_set_expand_to_uids_under_budget() {
    let set = parse_sequence_set("1:1000,2000:3000,5000").unwrap();
    let median = time_median(|| {
        let _ = sequence_set_to_uids(&set, 10_000);
    });
    // Budget: 1 ms. Observed P95: ~50 µs (expanding ~4000 UIDs).
    let budget = Duration::from_millis(1);
    assert!(
        median < budget,
        "sequence_set_to_uids median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn format_fetch_4_items_under_budget() {
    let items = vec![
        ("FLAGS".to_string(), "(\\Seen \\Recent)".to_string()),
        (
            "INTERNALDATE".to_string(),
            "\"20-May-2026 12:00:00 +0900\"".to_string(),
        ),
        ("RFC822.SIZE".to_string(), "4096".to_string()),
        ("UID".to_string(), "42".to_string()),
    ];
    let median = time_median(|| {
        let _ = format_fetch(1, &items);
    });
    // Budget: 50 µs. Observed P95: ~2 µs.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "format_fetch median {median:?} exceeded {budget:?}"
    );
}
