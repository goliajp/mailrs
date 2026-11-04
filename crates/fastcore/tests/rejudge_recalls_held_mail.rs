//! With acting on, a conversation the rules now hold is taken back into
//! Review, read or not: the reader gets a notice and a `mail.held` webhook,
//! and the recall is audited. An administrator puts a round back whole,
//! audited too, and a conversation a person put back is not taken again
//! for mail that was already there when they did.

mod rejudge_support;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use mailrs_core_sidestate::families::{audit, recall_notices, webhook_outbox, webhooks};
use mailrs_reputation::Stance;
use rejudge_support::*;

const KEVY: &str = "mem://fastcore-rejudge-recalls";

fn audit_rows(kv: &mut kevy_client::Connection, action: &str) -> Vec<audit::AuditFact> {
    kv.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<audit::AuditFact>(r).unwrap())
        .filter(|f| f.action == action)
        .collect()
}

#[tokio::test]
async fn a_held_verdict_takes_the_conversation_back_and_a_batch_puts_it_back() {
    // one test in its own integration-test process owns this variable
    unsafe { std::env::set_var("MAILRS_REJUDGE_ACT", "on") };
    let mut w = world(KEVY);
    let program = "prog@mail.vendorbill.com";
    let plain = "plain@mail.vendorbill.com";
    add(&w, program, &letter(program, true));
    add(&w, plain, &letter(plain, false));
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
    assert_eq!(round(&w.state).await.0, 0);

    // from the observation to the conversation in Review, the reader told
    let started = Instant::now();
    observe(&mut w.kv, Stance::Deny, "vendorbill.com");
    let (judged, counts) = round(&w.state).await;
    let took = started.elapsed();
    assert_eq!(judged, 2, "{counts}");
    assert!(took < Duration::from_secs(300), "{took:?}");
    eprintln!("observation to recall and notices: {took:?}");
    assert!(
        held(&w.state, program),
        "the read conversation is taken back"
    );
    assert!(
        !held(&w.state, plain),
        "a verdict that does not hold moves nothing"
    );
    assert_eq!(counts["acted"], 1, "{counts}");

    let now = chrono::Utc::now().timestamp();
    let notices = recall_notices::recent(&mut w.kv, USER, now).unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].thread_id, program);
    assert!(
        notices[0]
            .rules
            .iter()
            .any(|r| r == "executable-attachment"),
        "{notices:?}"
    );
    let observation = notices[0].observation.clone();

    let queued = webhook_outbox::claim_due(&mut w.kv, now + 1, 10).unwrap();
    assert_eq!(queued.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&queued[0].payload).unwrap();
    assert_eq!(body["event"], "mail.held", "{body}");
    assert_eq!(body["data"]["reason"], "rejudged", "{body}");
    assert_eq!(body["data"]["thread_id"], program, "{body}");

    let recalls = audit_rows(&mut w.kv, "rejudge.recall");
    assert_eq!(recalls.len(), 1);
    assert_eq!(recalls[0].target, program);
    assert!(recalls[0].detail.contains(&observation), "{:?}", recalls[0]);
    assert!(recalls[0].detail.contains("executable-attachment"));

    // the round is one batch
    let (status, list) = call(&w.state, Method::GET, "/v1/admin/rejudge-batches", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["items"][0]["observation"],
        observation.as_str(),
        "{list}"
    );
    assert_eq!(list["items"][0]["conversations"], 1, "{list}");
    assert_eq!(list["items"][0]["released_at"], 0, "{list}");

    let missing = call(
        &w.state,
        Method::POST,
        "/v1/admin/rejudge-batches/r999/release",
        Some(serde_json::json!({ "actor": "root@x.com" })),
    )
    .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
    let (status, done) = call(
        &w.state,
        Method::POST,
        &format!("/v1/admin/rejudge-batches/{observation}/release"),
        Some(serde_json::json!({ "actor": "root@x.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["released"], 1);
    assert!(!held(&w.state, program), "put back");
    let releases = audit_rows(&mut w.kv, "rejudge.release");
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].actor, "root@x.com");
    assert_eq!(releases[0].target, program);
    // what the batch was put back on: whose decision, which batch, the
    // rules it was held for, and how many it covered
    assert!(
        releases[0]
            .detail
            .contains(&format!("observation={observation}"))
            && releases[0].detail.contains("rules="),
        "{}",
        releases[0].detail
    );
    let scope = audit_rows(&mut w.kv, "rejudge.batch_release");
    assert_eq!(scope.len(), 1, "one line for the batch as a whole");
    assert_eq!(scope[0].actor, "root@x.com");
    assert_eq!(scope[0].target, observation);
    for want in [
        format!("batch={observation}"),
        "conversations=1".to_string(),
        "released=1".to_string(),
        "rules=".to_string(),
    ] {
        assert!(
            scope[0].detail.contains(&want),
            "{want} missing: {}",
            scope[0].detail
        );
    }
    assert!(
        !scope[0].detail.ends_with("rules="),
        "the batch names the rules it held for: {}",
        scope[0].detail
    );
    let fact = audit_rows(&mut w.kv, "quarantine.release");
    assert!(
        fact.iter().any(|f| f.target == program
            && f.actor == "root@x.com"
            && f.detail.contains("source=batch")
            && f.detail.contains(&format!("batch={observation}"))),
        "{:?}",
        fact.iter().map(|f| &f.detail).collect::<Vec<_>>()
    );
    let (_, list) = call(&w.state, Method::GET, "/v1/admin/rejudge-batches", None).await;
    assert!(
        list["items"][0]["released_at"].as_i64().unwrap() > 0,
        "{list}"
    );

    // the screening match goes away (an allow) and comes back (the allow
    // withdrawn): the verdict changes each time, the rules that hold it do
    // not, so it stays put
    let allow = observe(&mut w.kv, Stance::Allow, "vendorbill.com");
    for (stance, retracts) in [(Stance::Allow, None), (Stance::Retract, Some(allow))] {
        match retracts {
            None => {}
            Some(id) => {
                observe_retracting(&mut w.kv, stance, "vendorbill.com", Some(id));
            }
        }
        let before = counts_of(&w).await;
        let (_, after) = round(&w.state).await;
        assert!(
            after["verdicts_changed"].as_u64() > before["verdicts_changed"].as_u64(),
            "the verdict changed, so the hold was weighed: {after}"
        );
        assert!(
            !held(&w.state, program),
            "{stance:?}: taken again for the same rule"
        );
        assert_eq!(after["acted"], 1, "{after}");
    }
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 1);

    // put back when only the payment rule matched: the program is a rule
    // it was not put back despite, but a person put it back after this
    // mail came, so a new rule on the same mail does not take it again
    // (only mail that arrives after a person's release may)
    w.state
        .mailbox
        .store_ref()
        .hset(
            format!("rejudge:released:{USER}:{program}").as_bytes(),
            &[(
                b"rules".as_slice(),
                br#"["bec-pay-to-a-stranger"]"#.as_slice(),
            )],
        )
        .unwrap();
    let allow = observe(&mut w.kv, Stance::Allow, "vendorbill.com");
    let (_, after) = round(&w.state).await;
    assert!(
        !held(&w.state, program),
        "a new rule took back what a person released, on mail already there: {after}"
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 1);

    // the reader puts it back from Review: the same rules do not take it
    // a third time
    let (status, _) = call(
        &w.state,
        Method::POST,
        &format!("/v1/users/{USER}/quarantine/{program}/release"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!held(&w.state, program));
    observe_retracting(&mut w.kv, Stance::Retract, "vendorbill.com", Some(allow));
    let (_, after) = round(&w.state).await;
    assert!(
        !held(&w.state, program),
        "taken again after the reader put it back: {after}"
    );
    assert_eq!(audit_rows(&mut w.kv, "rejudge.recall").len(), 1);
}

async fn counts_of(w: &World) -> serde_json::Value {
    let (_, v) = call(
        &w.state,
        Method::GET,
        "/v1/admin/maintenance:rejudge-status",
        None,
    )
    .await;
    v
}
