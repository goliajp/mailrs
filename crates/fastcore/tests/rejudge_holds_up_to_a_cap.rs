//! One observation holds at most `MAILRS_REJUDGE_HOLD_CAP` conversations on
//! its own. Over the cap it holds nothing: the round is recorded as a
//! pending batch, with how many it would hold and for which rules, until a
//! super administrator confirms it (the recorded set is then held, weighed
//! against the conversations as they are by then) or discards it. At or
//! under the cap the conversations are held at once, and each reader gets
//! one notice and one push for the observation however many of theirs it
//! took; webhooks and audit lines stay one per conversation.

mod rejudge_support;

use axum::http::{Method, StatusCode};
use mailrs_core_sidestate::families::{audit, notify, recall_notices, webhook_outbox, webhooks};
use mailrs_reputation::Stance;
use rejudge_support::*;

const KEVY: &str = "mem://fastcore-rejudge-hold-cap";
const CAROL: &str = "carol@x.com";
const ROOT: &str = "root@x.com";

fn audit_rows(kv: &mut kevy_client::Connection, action: &str) -> Vec<audit::AuditFact> {
    kv.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<audit::AuditFact>(r).unwrap())
        .filter(|f| f.action == action)
        .collect()
}

/// The reader's notices as stored, newest first.
fn notices(kv: &mut kevy_client::Connection, user: &str) -> Vec<serde_json::Value> {
    kv.lrange(recall_notices::key(user).as_bytes(), 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect()
}

/// Web pushes attempted for `user` today. The subscription points at a
/// loopback address the sender refuses, so every attempt counts as failed.
fn pushes(kv: &mut kevy_client::Connection, user: &str) -> i64 {
    let now = chrono::Utc::now().timestamp();
    notify::stats(kv, user, 1, now).unwrap()[0]
        .counts
        .get("web")
        .and_then(|c| c.get("failed"))
        .copied()
        .unwrap_or(0)
}

/// Wait for the spawned pushes to be counted; `want` is reached or the
/// count after five seconds is returned.
async fn pushes_settled(kv: &mut kevy_client::Connection, user: &str, want: i64) -> i64 {
    for _ in 0..50 {
        if pushes(kv, user) >= want {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // a little longer, so a push too many would be counted too
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    pushes(kv, user)
}

fn vapid_key(dir: &std::path::Path) -> std::path::PathBuf {
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
    use base64::Engine as _;
    let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(doc.as_ref());
    let path = dir.join("vapid.pem");
    std::fs::write(
        &path,
        format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n"),
    )
    .unwrap();
    path
}

fn subscribe(kv: &mut kevy_client::Connection, user: &str) {
    notify::add_web_subscription(
        kv,
        user,
        &notify::WebSubscription {
            endpoint: format!("https://127.0.0.1/push/{user}"),
            p256dh: "BAAA".into(),
            auth: "AAAA".into(),
            created_at: 0,
        },
    )
    .unwrap();
}

/// `n` letters with a program from `from` to `user`, each its own
/// conversation; their ids.
fn letters(w: &World, user: &str, from: &str, n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| {
            let mid = format!("{i}.{}", from.replace('@', "."));
            add_as(w, user, from, &mid, &letter_from(from, user, &mid, true));
            mid
        })
        .collect()
}

async fn batch(w: &World, observation: &str) -> serde_json::Value {
    let (status, list) = call(&w.state, Method::GET, "/v1/admin/rejudge-batches", None).await;
    assert_eq!(status, StatusCode::OK);
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["observation"] == observation)
        .cloned()
        .unwrap_or_else(|| panic!("no batch {observation}: {list}"))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_cap_a_round_waits_for_a_decision_and_under_it_tells_each_reader_once() {
    let tmp = tempfile::tempdir().unwrap();
    let key = vapid_key(tmp.path());
    // one test in its own integration-test process owns these variables
    unsafe {
        std::env::set_var("MAILRS_REJUDGE_ACT", "on");
        std::env::set_var("MAILRS_REJUDGE_HOLD_CAP", "4");
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_VAPID_PRIVATE_KEY_PATH", &key);
        std::env::set_var("MAILRS_VAPID_SUBJECT", "mailto:ops@x.com");
    }
    let mut w = world(KEVY);
    reader(&w, CAROL);
    for user in [USER, CAROL] {
        subscribe(&mut w.kv, user);
    }
    webhooks::create(
        &mut w.kv,
        webhooks::NewWebhook {
            account_address: USER.into(),
            url: "https://hooks.test/held".into(),
            event_type: "mail.held".into(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )
    .unwrap();
    let over = letters(&w, USER, "billing@over.example", 5);
    let tossed = letters(&w, USER, "billing@tossed.example", 5);
    let under_bob = letters(&w, USER, "billing@under.example", 2);
    let under_carol = letters(&w, CAROL, "billing@under.example", 2);
    assert_eq!(round(&w.state).await.0, 0);
    let queued = |kv: &mut kevy_client::Connection| {
        webhook_outbox::claim_due(kv, chrono::Utc::now().timestamp() + 1, 100)
            .unwrap()
            .len()
    };

    // 1. five to hold, the cap is four: nothing is held, nobody is told,
    //    and the round waits as a pending batch
    let obs = observe(&mut w.kv, Stance::Deny, "over.example");
    let (judged, counts) = round(&w.state).await;
    assert_eq!(judged, 5, "{counts}");
    for mid in &over {
        assert!(!held(&w.state, mid), "{mid} held over the cap");
    }
    assert!(notices(&mut w.kv, USER).is_empty());
    assert_eq!(queued(&mut w.kv), 0, "no webhook over the cap");
    assert!(audit_rows(&mut w.kv, "rejudge.recall").is_empty());
    assert_eq!(pushes_settled(&mut w.kv, USER, 0).await, 0);
    let b = batch(&w, &obs).await;
    assert_eq!(b["pending"], true, "{b}");
    assert_eq!(b["would_hold"], 5, "{b}");
    assert_eq!(b["conversations"], 0, "{b}");
    assert_eq!(b["confirmed_at"], 0, "not confirmed yet: {b}");
    assert!(
        b["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "executable-attachment"),
        "{b}"
    );
    let pending = audit_rows(&mut w.kv, "rejudge.batch_pending");
    assert_eq!(pending.len(), 1);
    assert!(pending[0].detail.contains("would_hold=5"), "{pending:?}");
    // a pending batch holds nothing, so there is nothing to put back
    assert_eq!(decide(&w, &obs, "release").await.0, StatusCode::CONFLICT);

    // a person releases one of them meanwhile: confirming leaves it out
    w.state
        .mailbox
        .set_quarantined(USER, &over[0], true)
        .unwrap();
    let (status, _) = call(
        &w.state,
        Method::POST,
        &format!("/v1/users/{USER}/quarantine/{}/release", over[0]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 2. confirmed: the recorded set is held, the reader told once
    let (status, done) = decide(&w, &obs, "confirm").await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["held"], 4, "{done}");
    assert!(!held(&w.state, &over[0]), "a person released it");
    for mid in &over[1..] {
        assert!(held(&w.state, mid), "{mid}");
    }
    let told = notices(&mut w.kv, USER);
    assert_eq!(told.len(), 1, "one notice for the observation: {told:?}");
    assert_eq!(
        told[0]["thread_ids"].as_array().unwrap().len(),
        4,
        "{told:?}"
    );
    assert_eq!(pushes_settled(&mut w.kv, USER, 1).await, 1, "one push");
    assert_eq!(queued(&mut w.kv), 4, "a webhook per conversation");
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 4);
    let confirmed = audit_rows(&mut w.kv, "rejudge.batch_confirm");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(
        (confirmed[0].actor.as_str(), confirmed[0].target.as_str()),
        (ROOT, obs.as_str())
    );
    assert!(confirmed[0].detail.contains("held=4"), "{confirmed:?}");
    let b = batch(&w, &obs).await;
    assert_eq!(
        (b["pending"].clone(), b["conversations"].clone()),
        (false.into(), 4.into()),
        "{b}"
    );
    assert!(
        b["confirmed_at"].as_i64().is_some_and(|t| t > 0),
        "the list says when it was confirmed: {b}"
    );
    assert_eq!(
        decide(&w, &obs, "confirm").await.0,
        StatusCode::CONFLICT,
        "decided"
    );
    assert_eq!(decide(&w, "r999", "confirm").await.0, StatusCode::NOT_FOUND);

    // 3. discarded: nothing is held and nobody is told
    let obs = observe(&mut w.kv, Stance::Deny, "tossed.example");
    round(&w.state).await;
    assert_eq!(batch(&w, &obs).await["pending"], true);
    let (status, done) = decide(&w, &obs, "discard").await;
    assert_eq!(status, StatusCode::OK, "{done}");
    for mid in &tossed {
        assert!(!held(&w.state, mid), "{mid}");
    }
    let b = batch(&w, &obs).await;
    assert_eq!(b["pending"], false, "{b}");
    assert!(b["discarded_at"].as_i64().unwrap() > 0, "{b}");
    assert_eq!(
        b["confirmed_at"], 0,
        "a discarded batch was not confirmed: {b}"
    );
    assert_eq!(notices(&mut w.kv, USER).len(), 1);
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 4);
    let discarded = audit_rows(&mut w.kv, "rejudge.batch_discard");
    assert_eq!(discarded.len(), 1);
    assert_eq!(discarded[0].actor, ROOT);
    assert!(
        discarded[0].detail.contains("would_hold=5"),
        "{discarded:?}"
    );
    assert_eq!(decide(&w, &obs, "confirm").await.0, StatusCode::CONFLICT);
    assert_eq!(decide(&w, &obs, "discard").await.0, StatusCode::CONFLICT);

    // 4. four across two readers is at the cap: held at once, one notice and
    //    one push per reader
    let obs = observe(&mut w.kv, Stance::Deny, "under.example");
    round(&w.state).await;
    for mid in &under_bob {
        assert!(held_by(&w.state, USER, mid), "{mid}");
    }
    for mid in &under_carol {
        assert!(held_by(&w.state, CAROL, mid), "{mid}");
    }
    for (user, mids) in [(USER, &under_bob), (CAROL, &under_carol)] {
        let told = notices(&mut w.kv, user);
        let newest = &told[0];
        assert_eq!(newest["observation"], obs.as_str(), "{told:?}");
        let ids: Vec<&str> = newest["thread_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let mut want: Vec<&str> = mids.iter().map(String::as_str).collect();
        want.sort();
        let mut got = ids.clone();
        got.sort();
        assert_eq!(got, want, "{user}");
        assert_eq!(
            told.iter()
                .filter(|n| n["observation"] == obs.as_str())
                .count(),
            1,
            "{user}: {told:?}"
        );
    }
    assert_eq!(pushes_settled(&mut w.kv, USER, 2).await, 2, "bob: one more");
    assert_eq!(pushes_settled(&mut w.kv, CAROL, 1).await, 1, "carol: one");
    assert_eq!(
        queued(&mut w.kv),
        2,
        "bob's two conversations, a webhook each"
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 8);
    let b = batch(&w, &obs).await;
    assert_eq!(
        (b["pending"].clone(), b["conversations"].clone()),
        (false.into(), 4.into()),
        "{b}"
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.batch_pending").len(), 2);
}
