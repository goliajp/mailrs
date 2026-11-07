//! The owner's view of their sessions, ending them, and the mail the owner
//! gets when their account changes: a new device, a new recovery address,
//! two-step verification turned off.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const PW: &str = "pw";
const SAFARI_17: &str = "Mozilla/5.0 (Macintosh) Version/17.4 Safari/605.1.15";
const SAFARI_18: &str = "Mozilla/5.0 (Macintosh) Version/18.1 Safari/605.1.15";
const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0";

async fn login_as(live: &live::Live, user_agent: &str) -> String {
    let res = live
        .http
        .post(format!("{}/api/auth/login", live.base))
        .header("user-agent", user_agent)
        .json(&json!({ "address": ALICE, "password": PW }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

/// Security mail queued so far: (recipients, subject).
fn notices(kevy: &str) -> Vec<(String, String)> {
    use base64::Engine as _;
    use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
    let mut conn = kevy_client::Connection::connect(kevy).unwrap();
    let mut out: Vec<(String, String)> = conn
        .lrange(PENDING_IDX, 0, -1)
        .unwrap()
        .into_iter()
        .map(|id| {
            let id = String::from_utf8(id).unwrap();
            let blob = conn.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
            let job: Value = serde_json::from_slice(&blob).unwrap();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(job["message_data_b64"].as_str().unwrap())
                .unwrap();
            let subject = mailrs_mime::parse(&raw)
                .walk()
                .next()
                .and_then(|_| {
                    String::from_utf8_lossy(&raw)
                        .lines()
                        .find(|l| l.starts_with("Subject:"))
                        .map(str::to_string)
                })
                .unwrap_or_default();
            (job["recipient"].as_str().unwrap().to_string(), subject)
        })
        .collect();
    out.sort();
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_are_listed_and_ended_and_the_owner_is_told() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
        std::env::set_var("MAILRS_SECURITY_ALERT_LANG", "en");
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, PW).await;

    // the first device is recorded, not reported
    let first = login_as(&live, SAFARI_17).await;
    assert!(notices(&kevy).is_empty(), "{:?}", notices(&kevy));
    // a browser update is the same device
    let updated = login_as(&live, SAFARI_18).await;
    assert!(notices(&kevy).is_empty());
    // a new one is reported to the owner
    let other = login_as(&live, FIREFOX).await;
    let told = notices(&kevy);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, ALICE);
    assert!(told[0].1.contains("A new device signed in"), "{told:?}");

    let call = |token: &str, m: Method, p: &str, b: Option<Value>| {
        let (token, live) = (token.to_string(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &token, b).await }
    };
    let (status, list) = call(&other, Method::GET, "/api/auth/sessions", None).await;
    assert_eq!(status, 200, "{list}");
    let list = list.as_array().unwrap().clone();
    assert_eq!(list.len(), 3, "{list:?}");
    let current: Vec<&Value> = list.iter().filter(|s| s["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["user_agent"], FIREFOX);
    assert!(!current[0]["ip"].as_str().unwrap().is_empty());
    assert!(
        !list.iter().any(|s| s.to_string().contains(&first)),
        "a token was shown"
    );

    // end the first one by its id: it stops working, the others do not
    let first_id = list.iter().find(|s| s["user_agent"] == SAFARI_17).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // each sign-in's event names the session it started, by the listed id
    // and never by the token, so an ended session leads back to its sign-in
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let day = format!("auth:events:{}", mailrs_auth_guard::events::day(now));
    let raw = kevy_client::Connection::connect(&kevy)
        .unwrap()
        .lrange(day.as_bytes(), 0, -1)
        .unwrap();
    let mut started: Vec<String> = raw
        .iter()
        .map(|e| serde_json::from_slice::<Value>(e).unwrap())
        .filter(|e| e["result"] == "ok" && e["channel"] == "web")
        .map(|e| e["session_id"].as_str().unwrap().to_string())
        .collect();
    started.sort();
    let mut listed: Vec<String> = list
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    listed.sort();
    assert_eq!(started, listed);
    for token in [&first, &updated, &other] {
        assert!(
            !raw.iter()
                .any(|e| String::from_utf8_lossy(e).contains(token.as_str())),
            "a token was logged"
        );
    }

    let (status, _) = call(
        &other,
        Method::DELETE,
        &format!("/api/auth/sessions/{first_id}"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(&first, Method::GET, "/api/auth/me", None).await;
    assert_eq!(status, 401);
    let (status, _) = call(
        &other,
        Method::DELETE,
        "/api/auth/sessions/0000000000000000",
        None,
    )
    .await;
    assert_eq!(status, 404);

    // end the others: only this one is left
    let (status, ended) = call(&other, Method::POST, "/api/auth/sessions/end-others", None).await;
    assert_eq!((status, ended["ended"].as_u64()), (200, Some(1)));
    // each ended session is its own audit line, saying which it was: the
    // one ended by id above and the one end-others just ended
    let mut c = kevy_client::Connection::connect(&kevy).unwrap();
    let lines: Vec<Value> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<Value>(r).unwrap())
        .filter(|r| r["action"] == "auth.session.end")
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    let detail = lines[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.starts_with("id=") && detail.contains(" created=") && detail.contains(" ua="),
        "{detail}"
    );
    let (status, _) = call(&updated, Method::GET, "/api/auth/me", None).await;
    assert_eq!(status, 401);
    let (_, list) = call(&other, Method::GET, "/api/auth/sessions", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // a new recovery address: the account and the new address are told
    live.confirm(&other, PW).await;
    let (status, _) = call(
        &other,
        Method::POST,
        "/api/auth/recovery-email",
        Some(json!({ "recovery_email": "alice.backup@elsewhere.example" })),
    )
    .await;
    assert_eq!(status, 204);
    let told = notices(&kevy);
    let recovery: Vec<&(String, String)> = told
        .iter()
        .filter(|(_, s)| s.contains("recovery address was changed"))
        .collect();
    let mut to: Vec<&str> = recovery.iter().map(|(r, _)| r.as_str()).collect();
    to.sort();
    assert_eq!(to, ["alice.backup@elsewhere.example", ALICE], "{told:?}");
}
