//! Calendar rows filed before recurring events kept their series get one
//! from `backfill-invites?refile=true`.
//!
//! Those rows hold only the first occurrence, so an accepted weekly
//! meeting showed once and conflicted with nothing after its first week.
//! The invitations are already recorded on their messages, so the plain
//! backfill skips them; `refile` files their events again. The series is
//! filed before the mail that moved one of its occurrences, whatever
//! order the mailbox lists them in.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_core_sidestate::families::calendar_events::{self as ev, StoredEvent};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const KEVY: &str = "mem://fastcore-invite-refile";
const SERIES: &[u8] =
    include_bytes!("../../ical/tests/fixtures/itip/outlook/recurring-request.eml");
const MOVED: &[u8] =
    include_bytes!("../../ical/tests/fixtures/itip/outlook/recurring-instance-override.eml");
const UID: &str = "outlook-standup-mwf@contoso.com";
// an invitation whose row is gone (deleted over CalDAV, say)
const GONE: &[u8] = include_bytes!("../../ical/tests/fixtures/itip/outlook/request.eml");
const GONE_UID: &str = "outlook-roadmap-2026-q2@contoso.com";

/// One test per binary: the route reads `MAILRS_MAILDIR` and the state
/// reads `MAILRS_KEVY_URL` from the process environment.
#[tokio::test]
async fn an_old_row_gets_its_series_and_the_moved_occurrence() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    // the moved occurrence first, as a mailbox may list it
    let moved_ref = md.deliver(MOVED).expect("deliver").0;
    let series_ref = md.deliver(SERIES).expect("deliver").0;
    let gone_ref = md.deliver(GONE).expect("deliver").0;
    // SAFETY: own process, set before the state and router are built
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
    }

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    for (mid, blob_ref) in [
        ("outlook-rec-inst-001@contoso.com", moved_ref.as_str()),
        ("outlook-rec-001@contoso.com", series_ref.as_str()),
        ("outlook-req-001@contoso.com", gone_ref.as_str()),
    ] {
        mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: 1_786_000_000,
                latest_preview: "body",
                senders_csv: "organizer@contoso.com",
                subject: "Daily Standup",
                thread_id: "t-standup",
                unread: true,
                user: USER,
            })
            .expect("arrival");
        // already recorded as invitations, as every delivered one is
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": blob_ref,
            "sender": "organizer@contoso.com", "recipients": USER, "subject": "Daily Standup",
            "date": 1_786_000_000i64, "internal_date": 1_786_000_000i64, "size": 32,
            "flags": 0, "message_id": mid, "in_reply_to": "",
            "thread_id": "t-standup", "modseq": 1, "invite_method": "REQUEST",
        });
        mailbox
            .upsert_user_message(
                USER,
                "t-standup",
                mid,
                1_786_000_000,
                &serde_json::to_vec(&wire).expect("json"),
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref,
                    uid: 0,
                    flags: 0,
                    modseq: 1,
                },
            )
            .expect("seed");
    }
    // the row as it was filed before: the first occurrence, no series
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let old = StoredEvent {
        uid: UID.into(),
        summary: "Daily Standup".into(),
        dtstart: Some("2026-05-04T16:00:00+00:00".into()),
        dtend: Some("2026-05-04T16:15:00+00:00".into()),
        source: format!("mail:{UID}"),
        ..Default::default()
    };
    ev::put(&mut kv, USER, &old).expect("old row");
    ev::set_partstat(&mut kv, USER, UID, "ACCEPTED").expect("answer");

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let plain = backfill(&state, "").await;
    assert_eq!(plain["already_recorded"], 3, "{plain}");
    assert_eq!(
        plain["refiled"], 0,
        "the plain run leaves recorded ones alone: {plain}"
    );
    assert!(
        ev::get(&mut kv, USER, UID)
            .unwrap()
            .unwrap()
            .recurrence
            .is_none()
    );

    let dry = backfill(&state, "?refile=true&dry_run=true").await;
    assert_eq!(dry["refiled"], 2, "{dry}");
    assert_eq!(dry["refile_missing_row"], 1, "{dry}");
    assert!(
        ev::get(&mut kv, USER, UID)
            .unwrap()
            .unwrap()
            .recurrence
            .is_none()
    );

    let run = backfill(&state, "?refile=true").await;
    assert_eq!(run["refiled"], 2, "{run}");
    assert_eq!(run["refile_missing_row"], 1, "{run}");
    assert_eq!(run["changed"], 0, "{run}");
    assert!(
        ev::get(&mut kv, USER, GONE_UID).unwrap().is_none(),
        "a meeting whose row is gone is not brought back"
    );
    let row = ev::get(&mut kv, USER, UID).unwrap().unwrap();
    assert_eq!(
        row.partstat.as_deref(),
        Some("ACCEPTED"),
        "the answer stays"
    );
    let from = chrono::DateTime::parse_from_rfc3339("2026-05-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let to = chrono::DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z")
        .unwrap()
        .to_utc();
    let starts: Vec<String> = ev::series::occurrences(&row, from, to)
        .expect("a series now")
        .into_iter()
        .map(|o| o.start.to_column())
        .collect();
    // Monday, Wednesday and Friday at 09:00 Pacific, ten times; the
    // Friday 8 May one moved to 10:00
    assert_eq!(starts.len(), 10, "{starts:?}");
    assert_eq!(starts[0], "2026-05-04T16:00:00+00:00");
    assert_eq!(starts[2], "2026-05-08T17:00:00+00:00");
    assert_eq!(starts[9], "2026-05-25T16:00:00+00:00");
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/maintenance:backfill-invites{query}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}
