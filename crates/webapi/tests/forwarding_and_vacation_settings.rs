//! Forwarding and the vacation reply through the web: a new address needs
//! the password again and the code sent to it, and the code is never shown.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forwarding_waits_for_its_code_and_vacation_is_checked() {
    // the code goes out through the outbound queue, which a real
    // kevy-server takes (it is written with a pipeline)
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, PW).await;
    let alice = live.login(ALICE, PW).await;
    let call = |m: Method, p: &str, b: Option<Value>| {
        let (alice, live) = (alice.clone(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &alice, b).await }
    };

    // vacation
    let (status, v) = call(Method::GET, "/api/settings/vacation", None).await;
    assert_eq!((status, v["enabled"].as_bool()), (200, Some(false)));
    let away = json!({"enabled": true, "subject": "Away", "body": "Back Monday.", "start": null, "end": null, "days": 7});
    let (status, saved) = call(Method::PUT, "/api/settings/vacation", Some(away.clone())).await;
    assert_eq!(
        (status, saved["version"].as_u64()),
        (200, Some(1)),
        "{saved}"
    );
    let mut bad = away.clone();
    bad["days"] = json!(0);
    let (status, got) = call(Method::PUT, "/api/settings/vacation", Some(bad)).await;
    assert_eq!((status, got["error"].as_str()), (400, Some("invalid_days")));

    // forwarding: the password first
    let body = json!({"address": "Away@Elsewhere.example", "keep_copy": false});
    let (status, got) = call(Method::POST, "/api/settings/forwarding", Some(body.clone())).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("confirm_required"))
    );
    live.confirm(&alice, PW).await;
    let (status, got) = call(
        Method::POST,
        "/api/settings/forwarding",
        Some(json!({"address": "not an address"})),
    )
    .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (400, Some("invalid_address"))
    );
    let (status, got) = call(
        Method::POST,
        "/api/settings/forwarding",
        Some(json!({"address": ALICE})),
    )
    .await;
    assert_eq!((status, got["error"].as_str()), (400, Some("own_address")));
    let (status, _) = call(Method::POST, "/api/settings/forwarding", Some(body)).await;
    assert_eq!(status, 202);
    let (_, shown) = call(Method::GET, "/api/settings/forwarding", None).await;
    assert_eq!(shown["address"], "");
    assert_eq!(shown["pending_address"], "away@elsewhere.example");
    assert!(!shown.to_string().contains("code"), "{shown}");

    // the code went to the address, not to the page: read it off the mail
    // queued for that address
    let (status, got) = call(
        Method::POST,
        "/api/settings/forwarding/verify",
        Some(json!({"code": "not-it"})),
    )
    .await;
    assert_eq!((status, got["error"].as_str()), (400, Some("wrong_code")));
    assert_eq!(got["tries_left"], 4);
    let code = code_sent_to(&kevy, "away@elsewhere.example");
    let (status, got) = call(
        Method::POST,
        "/api/settings/forwarding/verify",
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(
        (status, got["address"].as_str()),
        (200, Some("away@elsewhere.example"))
    );
    let (_, shown) = call(Method::GET, "/api/settings/forwarding", None).await;
    assert_eq!(
        (shown["address"].as_str(), shown["keep_copy"].as_bool()),
        (Some("away@elsewhere.example"), Some(false))
    );

    let (status, _) = call(Method::DELETE, "/api/settings/forwarding", None).await;
    assert_eq!(status, 204);
    let (_, shown) = call(Method::GET, "/api/settings/forwarding", None).await;
    assert_eq!(shown["address"], "");
}

/// The six digits in the mail queued for `to`.
fn code_sent_to(kevy: &str, to: &str) -> String {
    use base64::Engine as _;
    use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
    let mut conn = kevy_client::Connection::connect(kevy).unwrap();
    let mail = conn
        .lrange(PENDING_IDX, 0, -1)
        .unwrap()
        .into_iter()
        .map(|id| {
            let id = String::from_utf8(id).unwrap();
            let blob = conn.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
            serde_json::from_slice::<Value>(&blob).unwrap()
        })
        .find(|job| job["recipient"] == to)
        .expect("a mail queued for the address");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(mail["message_data_b64"].as_str().unwrap())
        .unwrap();
    let root = mailrs_mime::parse(&raw);
    let text = root
        .find_by_content_type("text/plain")
        .and_then(|p| p.body_text())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let at = text.find("settings: ").expect("the code line") + "settings: ".len();
    text[at..at + 6].to_string()
}
