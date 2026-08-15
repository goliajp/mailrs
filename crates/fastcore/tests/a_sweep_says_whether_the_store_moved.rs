//! A sweep's numbers describe an instant, or they say they do not.
//!
//! Every reconcile and shadow route walks thirty thousand threads while
//! mail is arriving. A difference it reports may be a message that landed
//! mid-walk; a zero may be one that landed behind its cursor. Round 1 read
//! those zeros as proof that repairs had worked, and every one of them was
//! read off a live store.
//!
//! Freezing the store was considered and refused in the round-2 design:
//! the shadows compare the **maildir** against **kevy**, and a snapshot
//! freezes only the second — a message written to the maildir first, as
//! the write order requires, would then look like a difference that the
//! un-frozen version does not report.
//!
//! So the sweep says so instead. `changes_tail()` at entry and exit gives
//! how many writes landed while it ran: zero means it saw a still store
//! and its numbers are exact.
//!
//! **`still` must be able to be false, and must not be a disabled feed
//! wearing a zero.** A store with no feed cannot answer the question, and
//! saying "still" for it would be the shape this whole round exists to
//! remove: a metric that cannot come out other than clean.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";

fn store(feed: bool) -> mailrs_mailbox_kevy::KevyMailboxStore {
    let cfg = if feed {
        kevy_embedded::Config::default().with_feed(1 << 20)
    } else {
        kevy_embedded::Config::default()
    };
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(cfg).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s.upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    s
}

async fn shadow(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:read-state-shadow")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body"),
    )
    .expect("json")
}

/// Every sweep that walks the keyspace answers the question.
///
/// Listed rather than discovered, because a route added later is exactly
/// the one that will be forgotten — and a sweep that silently omits the
/// caveat reads as one that earned it.
const SWEEPS: &[&str] = &[
    "/v1/admin/maintenance:read-state-shadow",
    "/v1/admin/maintenance:read-state-backfill?dry_run=true",
    "/v1/admin/maintenance:usermsg-shadow",
    "/v1/admin/maintenance:threadrow-shadow",
    "/v1/admin/maintenance:sent-axis-shadow",
    "/v1/admin/maintenance:shadow-counts",
    "/v1/admin/maintenance:count-shadow",
    "/v1/admin/maintenance:group-backfill?dry_run=true",
    "/v1/admin/maintenance:axis-shadow",
    "/v1/admin/maintenance:reindex?dry_run=true",
    "/v1/admin/maintenance:uidlist-backfill",
    "/v1/admin/maintenance:allocate-missing-uids",
];

#[tokio::test]
async fn every_sweep_reports_whether_the_store_was_still() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store(true)));

    for uri in SWEEPS {
        let res = mailrs_fastcore::build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(*uri)
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("call");
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), 1 << 20)
                .await
                .expect("body"),
        )
        .expect("json");
        assert!(
            body.get("still").is_some(),
            "{uri} walks the keyspace and does not say whether it moved: {body}"
        );
    }
}

#[tokio::test]
async fn a_still_store_is_reported_as_still_and_a_moving_one_is_not() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    // ── a store nothing is writing to ──
    let quiet = Arc::new(mailrs_fastcore::FastcoreState::new(store(true)));
    let report = shadow(&quiet).await;
    assert_eq!(
        report["still"], true,
        "nothing wrote during this sweep and it did not say so: {report}"
    );
    assert_eq!(report["writes_during"], 0, "{report}");

    // ── a store being written to while the sweep runs ──
    //
    // The route is synchronous, so the write goes in between the two
    // cursor reads by driving them directly — the same helper the route
    // uses, which is what makes this a test of the helper rather than of
    // a sleep.
    let busy = Arc::new(mailrs_fastcore::FastcoreState::new(store(true)));
    let motion = mailrs_fastcore::store_motion_probe(&busy, || {
        busy.mailbox
            .upsert_account("late@x.com", r#"{"address":"late@x.com"}"#)
            .expect("a write lands mid-sweep");
    });
    assert!(
        motion.0 > 0 && !motion.1,
        "a write landed during the sweep and it reported still: {motion:?}"
    );

    // ── a store whose feed is off cannot answer ──
    //
    // And must not answer "still". A disabled feed reporting a clean
    // sweep is a number that cannot come out dirty, which is the defect
    // this round is named after.
    let blind = Arc::new(mailrs_fastcore::FastcoreState::new(store(false)));
    let report = shadow(&blind).await;
    assert!(
        report["still"].is_null(),
        "a store that cannot see its own writes claimed the sweep was \
         still: {report}"
    );
}
