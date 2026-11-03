//! An all-day event is stored as its dates and the calendar lists it for
//! a reader in any zone whose days it touches; it is never a conflict.
//! Before, its start was read as an instant, a date did not parse as one,
//! and the event was left out of every listing.

mod live;

use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent};
use reqwest::Method;

const KEVY: &str = "mem://webapi-all-day-events";
const ALICE: &str = "alice@example.test";

async fn uids(live: &live::Live, path: &str, bearer: &str) -> Vec<String> {
    let (status, body) = live.call(Method::GET, path, bearer, None).await;
    assert_eq!(status, 200, "{path}: {body}");
    body.as_array()
        .expect("array")
        .iter()
        .map(|r| r["uid"].as_str().expect("uid").to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_all_day_event_is_listed_on_its_days_and_conflicts_with_nothing() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;

    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let holiday = StoredEvent {
        uid: "holiday".into(),
        summary: "Holiday".into(),
        dtstart: Some("2026-09-01".into()),
        dtend: Some("2026-09-02".into()),
        source: "feed:jp-holidays".into(),
        ..Default::default()
    };
    calendar_events::put(&mut kv, ALICE, &holiday).expect("put");

    // 1 September in Tokyo (UTC+9) and in Honolulu (UTC−10)
    for window in [
        "start=2026-08-31T15:00:00Z&end=2026-09-01T15:00:00Z",
        "start=2026-09-01T10:00:00Z&end=2026-09-02T10:00:00Z",
    ] {
        assert_eq!(
            uids(&live, &format!("/api/calendar/events?{window}"), &alice).await,
            ["holiday"],
            "{window}"
        );
    }
    // a week later is not its day anywhere
    assert!(
        uids(
            &live,
            "/api/calendar/events?start=2026-09-08T00:00:00Z&end=2026-09-09T00:00:00Z",
            &alice
        )
        .await
        .is_empty()
    );
    // a day off blocks no meeting on it
    assert!(
        uids(
            &live,
            "/api/calendar/conflicts?start=2026-09-01T02:00:00Z&end=2026-09-01T03:00:00Z",
            &alice
        )
        .await
        .is_empty()
    );
}
