use std::collections::BTreeMap;

use mailrs_ops_report::wire::{CspEntry, Detail, Report, ReportKind, WebapiBeat};

use super::*;
use crate::store::Store;

const DAY: i64 = 86_400;
const NOW: i64 = 1_800_000_000;

fn entry(blocked: &str, count: u64, first_at: i64, last_at: i64) -> CspEntry {
    CspEntry {
        directive: "img-src".into(),
        violated_directive: "img-src".into(),
        blocked: blocked.into(),
        page: "mail".into(),
        client: "desktop".into(),
        count,
        first_at,
        last_at,
    }
}

fn beat(reports: u64, distinct: Vec<CspEntry>, pages: u64) -> WebapiBeat {
    WebapiBeat {
        csp_reports: reports,
        csp_by_directive: BTreeMap::from([("img-src".into(), reports)]),
        csp_distinct: distinct,
        csp_distinct_dropped: 0,
        pages_served: BTreeMap::from([("desktop".into(), pages)]),
    }
}

fn heartbeat(store: &Store, instance: &str, start: u64, seq: u64, at: i64, b: WebapiBeat) {
    let r = Report {
        process: "webapi".into(),
        instance: instance.into(),
        build: mailrs_ops_report::build::build(),
        start_count: Some(start),
        seq,
        kind: ReportKind::Heartbeat,
        sent_at: at,
        kevy_contact_at: None,
        detail: Some(Detail::Webapi(b)),
    };
    store.record_report(&r, at).unwrap();
}

#[test]
fn nothing_is_counted_before_the_first_heartbeat() {
    let store = Store::in_memory().unwrap();
    let s = summary(&store, NOW).unwrap();
    assert_eq!(s.counting_since, None);
    assert_eq!(s.last_7_days, None);
    assert_eq!(s.since_counting, None);
}

/// Instance a restarts inside the window and its counts go back to zero;
/// instance b runs beside it. Each start counts only what it added inside
/// the window.
#[test]
fn a_window_sums_what_each_start_added_inside_it() {
    let store = Store::in_memory().unwrap();
    let from = NOW - WINDOW_SECS;
    // a, first start: 5 reports before the window, 8 by its last beat inside
    heartbeat(
        &store,
        "a",
        1,
        1,
        from - DAY,
        beat(5, vec![entry("https://x", 5, from - DAY, from - DAY)], 50),
    );
    heartbeat(
        &store,
        "a",
        1,
        2,
        from + DAY,
        beat(8, vec![entry("https://x", 8, from - DAY, from + DAY)], 70),
    );
    // a, second start: counts from zero
    heartbeat(
        &store,
        "a",
        2,
        1,
        from + 2 * DAY,
        beat(
            1,
            vec![entry("https://x", 1, from + 2 * DAY, from + 2 * DAY)],
            4,
        ),
    );
    heartbeat(
        &store,
        "a",
        2,
        2,
        NOW - 60,
        beat(
            2,
            vec![
                entry("https://x", 1, from + 2 * DAY, from + 2 * DAY),
                entry("https://y", 1, NOW - 60, NOW - 60),
            ],
            9,
        ),
    );
    // b: started before the window and never beat inside it
    heartbeat(
        &store,
        "b",
        1,
        1,
        from - 2 * DAY,
        beat(100, vec![entry("https://z", 100, 0, 0)], 1000),
    );
    // c: started inside the window
    heartbeat(
        &store,
        "c",
        1,
        1,
        NOW - 120,
        beat(3, vec![entry("https://y", 3, NOW - 300, NOW - 120)], 6),
    );

    let s = summary(&store, NOW).unwrap();
    assert_eq!(s.counting_since, Some(from - 2 * DAY));

    let w = s.last_7_days.unwrap();
    // a1: 8-5, a2: 2, b: nothing inside, c: 3
    assert_eq!(w.reports, 3 + 2 + 3);
    assert_eq!(w.by_directive["img-src"], 8);
    assert_eq!(w.pages_served["desktop"], 20 + 9 + 6);
    let by: BTreeMap<&str, &CspEntry> =
        w.distinct.iter().map(|e| (e.blocked.as_str(), e)).collect();
    assert_eq!(by.len(), 2, "z added nothing inside the window");
    assert_eq!(by["https://x"].count, 3 + 1);
    assert_eq!(by["https://y"].count, 1 + 3);
    assert_eq!(by["https://y"].first_at, NOW - 300);
    assert_eq!(by["https://y"].last_at, NOW - 60);
    assert_eq!(w.distinct[0].count, 4);

    let all = s.since_counting.unwrap();
    assert_eq!(all.from, from - 2 * DAY);
    assert_eq!(all.reports, 8 + 2 + 100 + 3);
    assert_eq!(
        all.distinct
            .iter()
            .find(|e| e.blocked == "https://z")
            .map(|e| e.count),
        Some(100)
    );
}

#[test]
fn heartbeats_of_other_processes_and_without_counts_are_left_out() {
    let store = Store::in_memory().unwrap();
    let mut r = crate::readings::fixture::report("webapi", NOW - 10, None);
    store.record_report(&r, NOW - 10).unwrap();
    r.process = "sender".into();
    store.record_report(&r, NOW - 5).unwrap();
    assert_eq!(summary(&store, NOW).unwrap().counting_since, None);
}
