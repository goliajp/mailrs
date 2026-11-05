//! A round that held nothing of mail the rules hold — its batch discarded,
//! or judged while acting was off — leaves that mail's verdict reading
//! "hold". A later, different observation naming it offers it again,
//! although the verdict did not change; the round that left it unheld never
//! offers it again. A discarded batch holds nothing, so it cannot be put
//! back: like a pending one it answers 409 and writes nothing.

mod rejudge_support;

use axum::http::{Method, StatusCode};
use mailrs_core_sidestate::families::audit;
use mailrs_reputation::Stance;
use rejudge_support::*;

const KEVY: &str = "mem://fastcore-rejudge-offers-unheld-again";
const ROOT: &str = "root@x.com";

fn audit_rows(kv: &mut kevy_client::Connection, action: &str) -> Vec<audit::AuditFact> {
    kv.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<audit::AuditFact>(r).unwrap())
        .filter(|f| f.action == action)
        .collect()
}

/// `n` letters with a program from `from`, each its own conversation.
fn letters(w: &World, from: &str, n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| {
            let mid = format!("{i}.{}", from.replace('@', "."));
            add_as(w, USER, from, &mid, &letter_from(from, USER, &mid, true));
            mid
        })
        .collect()
}

async fn batch(w: &World, observation: &str) -> Option<serde_json::Value> {
    let (status, list) = call(&w.state, Method::GET, "/v1/admin/rejudge-batches", None).await;
    assert_eq!(status, StatusCode::OK);
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["observation"] == observation)
        .cloned()
}

async fn decide(w: &World, observation: &str, what: &str) -> (StatusCode, serde_json::Value) {
    call(
        &w.state,
        Method::POST,
        &format!("/v1/admin/rejudge-batches/{observation}/{what}"),
        Some(serde_json::json!({ "actor": ROOT })),
    )
    .await
}

fn set_env(name: &str, value: &str) {
    // one test in its own integration-test process owns these variables
    unsafe { std::env::set_var(name, value) };
}

/// Judge `observation` again, as a round would after its place was lost.
async fn judge_again(w: &World, observation: &str) -> u64 {
    let n: u64 = observation.trim_start_matches('r').parse().unwrap();
    w.state
        .mailbox
        .store_ref()
        .set(b"rejudge:obs_seq", (n - 1).to_string().as_bytes())
        .unwrap();
    round(&w.state).await.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_a_round_left_unheld_is_offered_again_by_a_later_observation_only() {
    set_env("MAILRS_REJUDGE_ACT", "off");
    set_env("MAILRS_REJUDGE_HOLD_CAP", "2");
    set_env("MAILRS_KEVY_URL", KEVY);
    let mut w = world(KEVY);
    let shadowed = letters(&w, "billing@shadow.example", 2);
    let tossed = letters(&w, "billing@tossed.example", 3);
    assert_eq!(round(&w.state).await.0, 0);

    // 1. judged while acting is off: the verdict is recorded, nothing held
    let first = observe(&mut w.kv, Stance::Deny, "shadow.example");
    assert_eq!(round(&w.state).await.0, 2);
    for mid in &shadowed {
        assert!(!held(&w.state, mid), "{mid} held in shadow");
    }
    assert!(batch(&w, &first).await.is_none());

    // acting on: the same observation judged again still holds nothing
    set_env("MAILRS_REJUDGE_ACT", "on");
    assert_eq!(judge_again(&w, &first).await, 2);
    for mid in &shadowed {
        assert!(!held(&w.state, mid), "{mid} offered again for {first}");
    }
    assert!(batch(&w, &first).await.is_none());

    // a later observation about the sender holds them, though no verdict
    // changed
    let later = observe(&mut w.kv, Stance::Deny, "shadow.example");
    let (judged, counts) = round(&w.state).await;
    assert_eq!(judged, 2, "{counts}");
    for mid in &shadowed {
        assert!(held(&w.state, mid), "{mid} not offered again by {later}");
    }
    let b = batch(&w, &later).await.expect("the later round's batch");
    assert_eq!(
        (b["pending"].clone(), b["conversations"].clone()),
        (false.into(), 2.into()),
        "{b}"
    );

    // 2. over the cap, discarded: nothing held
    let tossing = observe(&mut w.kv, Stance::Deny, "tossed.example");
    assert_eq!(round(&w.state).await.0, 3);
    assert_eq!(batch(&w, &tossing).await.unwrap()["pending"], true);
    let (status, done) = decide(&w, &tossing, "discard").await;
    assert_eq!(
        (status, done["would_hold"].clone()),
        (StatusCode::OK, 3.into())
    );
    for mid in &tossed {
        assert!(!held(&w.state, mid), "{mid}");
    }

    // a discarded batch holds nothing to put back: refused, nothing written
    let before = audit_rows(&mut w.kv, "rejudge.batch_release").len();
    assert_eq!(
        decide(&w, &tossing, "release").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.batch_release").len(), before);
    let b = batch(&w, &tossing).await.unwrap();
    assert_eq!(b["released_at"], 0, "{b}");
    assert!(b["discarded_at"].as_i64().unwrap() > 0, "{b}");

    // the discarding observation judged again proposes nothing anew
    assert_eq!(judge_again(&w, &tossing).await, 3);
    let b = batch(&w, &tossing).await.unwrap();
    assert_eq!(
        (b["pending"].clone(), b["conversations"].clone()),
        (false.into(), 0.into()),
        "{b}"
    );
    assert!(b["discarded_at"].as_i64().unwrap() > 0, "{b}");
    assert_eq!(audit_rows(&mut w.kv, "rejudge.batch_pending").len(), 1);
    for mid in &tossed {
        assert!(!held(&w.state, mid), "{mid} re-proposed for {tossing}");
    }

    // a later, different observation makes them candidates again
    set_env("MAILRS_REJUDGE_HOLD_CAP", "10");
    let again = observe(&mut w.kv, Stance::Deny, "tossed.example");
    assert_eq!(round(&w.state).await.0, 3);
    for mid in &tossed {
        assert!(held(&w.state, mid), "{mid} not offered by {again}");
    }
    let b = batch(&w, &again).await.expect("the later round's batch");
    assert_eq!(
        (b["pending"].clone(), b["conversations"].clone()),
        (false.into(), 3.into()),
        "{b}"
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 5);

    // held now, they are no longer left unheld: another observation offers
    // nothing
    let after = observe(&mut w.kv, Stance::Deny, "tossed.example");
    round(&w.state).await;
    assert!(batch(&w, &after).await.is_none());
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 5);
}
