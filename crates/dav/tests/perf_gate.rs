//! Performance regression gates.
//!
//! Each test runs a hot path a fixed number of iterations and asserts the
//! median elapsed time is under a documented budget. Budgets are set with
//! ample CI headroom — the goal is to catch order-of-magnitude regressions
//! (10× slower than expected), not measure performance precisely.
//!
//! Run `cargo bench -p mailrs-dav` for the full timing detail; perf_gate
//! is the cheap version that fails CI on regression.
//!
//! See [BUDGETS.md](../BUDGETS.md) for budget derivation and re-measurement
//! protocol.

use std::time::{Duration, Instant};

use mailrs_dav::caldav::{calendar_propfind, calendar_report};
use mailrs_dav::fixtures::{EXAMPLE_USER, InMemoryCalendarStore, make_calendar, make_event};
use mailrs_dav::principal::principal_propfind;

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

async fn time_median_async<F, Fut>(mut op: F) -> Duration
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op().await;
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

#[test]
fn principal_propfind_under_budget() {
    let median = time_median(|| {
        let _ = principal_propfind(EXAMPLE_USER, "");
    });

    // Budget: 100 µs. Observed P95: ~5 µs (criterion). ~20× headroom.
    let budget = Duration::from_micros(100);
    assert!(
        median < budget,
        "principal_propfind median {median:?} exceeded budget {budget:?}"
    );
}

#[tokio::test]
async fn calendar_propfind_depth_1_50_events_under_budget() {
    let mut store =
        InMemoryCalendarStore::new().with_calendar(EXAMPLE_USER, make_calendar(10, "Work"));
    for i in 0..50 {
        store = store.with_event(
            10,
            make_event(
                &format!("evt-{i}"),
                &format!("BEGIN:VEVENT\nUID:evt-{i}\nEND:VEVENT"),
            ),
        );
    }

    let median = time_median_async(|| async {
        let _ = calendar_propfind(&store, EXAMPLE_USER, "Work", 10, 1).await;
    })
    .await;

    // Budget: 5 ms. Observed P95: ~250 µs (criterion). ~20× headroom.
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "calendar_propfind_depth_1 (50 events) median {median:?} exceeded budget {budget:?}"
    );
}

#[tokio::test]
async fn calendar_report_multiget_50_under_budget() {
    let mut store =
        InMemoryCalendarStore::new().with_calendar(EXAMPLE_USER, make_calendar(10, "Work"));
    for i in 0..50 {
        store = store.with_event(
            10,
            make_event(
                &format!("evt-{i}"),
                &format!("BEGIN:VEVENT\nUID:evt-{i}\nSUMMARY:meeting {i}\nEND:VEVENT"),
            ),
        );
    }

    let multiget_body = (0..50)
        .map(|i| format!("<D:href>/dav/calendars/{EXAMPLE_USER}/Work/evt-{i}.ics</D:href>"))
        .collect::<String>();
    let report_body = format!(
        "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">{multiget_body}</C:calendar-multiget>"
    );

    let median = time_median_async(|| async {
        let _ = calendar_report(&store, EXAMPLE_USER, "Work", 10, &report_body).await;
    })
    .await;

    // Budget: 10 ms. Observed P95: ~600 µs (criterion). ~16× headroom.
    // multiget builds the full multistatus body, the largest typical payload
    // in CalDAV traffic.
    let budget = Duration::from_millis(10);
    assert!(
        median < budget,
        "calendar_report_multiget (50 events) median {median:?} exceeded budget {budget:?}"
    );
}
