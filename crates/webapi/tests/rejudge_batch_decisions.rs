//! A re-judge round that would hold more than one observation may on its
//! own waits for a super administrator: confirming or discarding it takes
//! a recent password confirmation, like putting a batch back, and both the
//! core and this tier audit the decision. A successful confirmation is
//! audited too; a failed one is counted by the login guard, as a failed
//! sign-in is, and writes no audit line.
//!
//! One test function: the binary shares one in-process store.

mod live;

use mailrs_core_sidestate::families::audit::AuditFact;
use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-rejudge-batch-decisions";
const SUPER: &str = "super@example.test";
const PLAIN: &str = "plain@example.test";
const PW: &str = "pw";

fn audit_rows(action: &str) -> Vec<AuditFact> {
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    kv.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit log")
        .iter()
        .map(|r| serde_json::from_slice::<AuditFact>(r).expect("audit row"))
        .filter(|f| f.action == action)
        .collect()
}

/// A round recorded as pending, as the core writes one over the cap.
fn seed_pending(live: &live::Live, observation: &str) {
    let entry = json!({
        "at": 1_788_000_000, "released_at": 0, "pending": true,
        "would_hold": 60, "rules": ["executable-attachment"], "discarded_at": 0,
    })
    .to_string();
    live.mailbox
        .store_ref()
        .hset(
            b"rejudge:batches",
            &[(observation.as_bytes(), entry.as_bytes())],
        )
        .expect("seed batch");
}

fn error_of(body: &Value) -> &str {
    body["error"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deciding_on_a_waiting_round_takes_a_confirmed_super_administrator() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [SUPER, PLAIN] {
        live.add_account(who, PW).await;
    }
    live.make_super(SUPER);
    seed_pending(&live, "r1");
    seed_pending(&live, "r2");
    let path = |obs: &str, what: &str| format!("/api/admin/rejudge/batches/{obs}/{what}");

    // the round is listed as waiting
    let sup = live.login(SUPER, PW).await;
    let (status, list) = live
        .call(Method::GET, "/api/admin/rejudge/batches", &sup, None)
        .await;
    assert_eq!(status, 200, "{list}");
    let r1 = list["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|b| b["observation"] == "r1")
        .cloned()
        .expect("r1 listed");
    assert_eq!(
        (r1["pending"].clone(), r1["would_hold"].clone()),
        (json!(true), json!(60)),
        "{r1}"
    );

    // not a super administrator: refused
    let plain = live.login(PLAIN, PW).await;
    for what in ["confirm", "discard"] {
        let (status, _) = live
            .call(Method::POST, &path("r1", what), &plain, None)
            .await;
        assert_eq!(status, 403, "plain {what}");
    }

    // a super administrator who has not confirmed: refused, saying why
    for what in ["confirm", "discard"] {
        let (status, got) = live.call(Method::POST, &path("r1", what), &sup, None).await;
        assert_eq!(
            (status, error_of(&got)),
            (403, "confirm_required"),
            "{what}"
        );
    }
    assert!(audit_rows("rejudge.batch_confirm").is_empty());
    assert!(audit_rows("rejudge.batch_discard").is_empty());

    // a failed confirmation writes no audit line; a successful one does
    let (status, _) = live
        .call(
            Method::POST,
            "/api/auth/confirm",
            &plain,
            Some(json!({ "password": "wrong" })),
        )
        .await;
    assert_eq!(status, 401);
    assert!(
        audit_rows("auth.confirm").is_empty(),
        "a failure is not audited"
    );
    live.confirm(&sup, PW).await;
    let confirmed = audit_rows("auth.confirm");
    assert_eq!(confirmed.len(), 1, "{confirmed:?}");
    assert_eq!(
        (confirmed[0].actor.as_str(), confirmed[0].target.as_str()),
        (SUPER, SUPER)
    );

    // confirmed: the decisions go through, each audited by the core and here
    let (status, got) = live
        .call(Method::POST, &path("r1", "confirm"), &sup, None)
        .await;
    assert_eq!((status, got.clone()), (200, json!({ "held": 0 })), "{got}");
    let rows = audit_rows("rejudge.batch_confirm");
    assert_eq!(rows.len(), 2, "core and webapi: {rows:?}");
    assert!(
        rows.iter().all(|r| r.actor == SUPER && r.target == "r1"),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.detail.contains("would_hold=60")),
        "{rows:?}"
    );
    let (status, got) = live
        .call(Method::POST, &path("r2", "discard"), &sup, None)
        .await;
    assert_eq!(
        (status, got.clone()),
        (200, json!({ "would_hold": 60 })),
        "{got}"
    );
    let rows = audit_rows("rejudge.batch_discard");
    assert_eq!(rows.len(), 2, "core and webapi: {rows:?}");
    assert!(
        rows.iter().all(|r| r.actor == SUPER && r.target == "r2"),
        "{rows:?}"
    );

    // decided rounds are listed as such and cannot be decided again
    let (_, list) = live
        .call(Method::GET, "/api/admin/rejudge/batches", &sup, None)
        .await;
    for b in list["items"].as_array().expect("items") {
        assert_eq!(b["pending"], false, "{b}");
    }
    for (obs, what) in [("r1", "confirm"), ("r2", "confirm"), ("r2", "discard")] {
        let (status, _) = live.call(Method::POST, &path(obs, what), &sup, None).await;
        assert_eq!(status, 409, "{obs} {what}");
    }
    let (status, _) = live
        .call(Method::POST, &path("r999", "confirm"), &sup, None)
        .await;
    assert_eq!(status, 404);
}
