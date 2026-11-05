//! A re-judge round over the hold cap waits as a pending batch: what it
//! would hold is kept as gathered conversations, and the batch says how
//! many. Purging a reader takes that reader's gathered conversations and
//! lowers the count by as many; a batch left with none is dropped, and
//! confirming or discarding it then answers 404, as for any batch that is
//! not there.
//!
//! Before, the count stayed as it was, and a batch whose readers were all
//! purged waited for a decision about nothing.

mod purge_support;

use purge_support::*;

const OBSERVATION: &str = "obs-pending";
const BATCHES: &[u8] = b"rejudge:batches";

/// The pending batch as `settle` records it.
fn pending(mail: &mut kevy_client::Connection, would_hold: u64) {
    let entry = serde_json::json!({ "at": 1_790_000_000, "released_at": 0, "pending": true,
        "would_hold": would_hold, "rules": ["r1"], "discarded_at": 0 })
    .to_string();
    mail.hset(BATCHES, &[(OBSERVATION.as_bytes(), entry.as_bytes())])
        .unwrap();
}

/// The batch's count, `None` when it is gone.
fn would_hold(mail: &mut kevy_client::Connection) -> Option<u64> {
    let raw = mail.hget(BATCHES, OBSERVATION.as_bytes()).unwrap()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    v["would_hold"].as_u64()
}

async fn delete(f: &Fixture, who: &str) {
    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/v1/admin/accounts/{who}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
}

#[tokio::test]
async fn a_pending_batch_loses_the_purged_readers_share_and_goes_when_none_is_left() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "0") };
    let f = fixture().await;
    let mut mail = mail_conn(&f.state);
    let key = format!("rejudge:candidates:{OBSERVATION}");
    for field in [
        format!("{BOB}\tt-bob-1"),
        format!("{BOB}\tt-bob-2"),
        format!("{ALICE}\tt-alice"),
    ] {
        mail.hset(key.as_bytes(), &[(field.as_bytes(), b"{}".as_slice())])
            .unwrap();
    }
    pending(&mut mail, 3);

    delete(&f, BOB).await;
    assert_eq!(would_hold(&mut mail), Some(1), "bob's two still counted");

    delete(&f, ALICE).await;
    assert_eq!(
        would_hold(&mut mail),
        None,
        "a batch of nothing is still waiting"
    );
    for decision in ["confirm", "discard"] {
        let (status, _) = call(
            &f.state,
            "POST",
            &format!("/v1/admin/rejudge-batches/{OBSERVATION}/{decision}"),
            Some(serde_json::json!({ "actor": "admin@x.com" })),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "{decision}");
    }
}
