//! Masked addresses through the front door, against a real fastcore and a
//! real kevy-server (the queue write is pipelined, which the in-process
//! kevy does not support).
//!
//! A mask delivers to its owner until it is turned off; an alias cannot be
//! made over it; a message sent from it names the mask in the envelope and
//! nowhere names the account; JMAP `MaskedEmail/*` makes and lists masks;
//! and a key that may only manage masks reaches nothing else.

mod live;

use base64::Engine as _;
use mailrs_core_sidestate::families::outbound as ob;
use reqwest::Method;
use serde_json::{Value, json};

const OWNER: &str = "owner@example.test";

/// Every queued job for `send_id`: (envelope sender, message bytes).
fn queued(kevy: &str, send_id: &str) -> Vec<(String, Vec<u8>)> {
    let mut c = kevy_client::Connection::connect(kevy).expect("kevy");
    let mut out = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, keys) = c
            .scan(cursor, Some(ob::JOB_PATTERN), Some(100))
            .expect("scan");
        for k in keys {
            let blob = c.hget(&k, b"blob").expect("hget").unwrap_or_default();
            let job: Value = serde_json::from_slice(&blob).expect("job blob");
            if job["send_id"] == send_id {
                let raw = base64::engine::general_purpose::STANDARD
                    .decode(job["message_data_b64"].as_str().expect("message"))
                    .expect("base64");
                out.push((job["sender"].as_str().expect("sender").to_string(), raw));
            }
        }
        if next == 0 {
            break;
        }
        cursor = next;
    }
    out
}

/// The header block of `raw`.
fn headers(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mask_delivers_sends_without_the_account_and_is_kept_apart() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(OWNER, "pw").await;
    let session = live.login(OWNER, "pw").await;

    // made, listed, and delivering to its owner
    let (status, made) = live
        .call(
            Method::POST,
            "/api/masked-emails",
            &session,
            Some(json!({ "forDomain": "shop.example", "description": "orders" })),
        )
        .await;
    assert_eq!(status, 201, "{made}");
    let mask = made["email"].as_str().expect("email").to_string();
    assert!(mask.ends_with("@example.test"), "{mask}");
    assert_eq!(made["state"], "enabled");
    let (_, list) = live
        .call(Method::GET, "/api/masked-emails", &session, None)
        .await;
    assert_eq!(list[0]["email"], mask.as_str());
    assert!(live.core.recipient_deliverable(&mask).await.expect("check"));

    // an alias over a mask is refused, even to one who may make aliases
    live.grant(OWNER, "admin.aliases").await;
    live.confirm(&session, "pw").await;
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/aliases",
            &session,
            Some(json!({ "source_address": mask, "target_address": OWNER })),
        )
        .await;
    assert_eq!(status, 409, "an alias over a mask");

    // a message from the mask names the mask in the envelope, and neither
    // the account's address nor its local part anywhere in the headers
    let (status, sent) = live
        .call(
            Method::POST,
            "/api/mail/send",
            &session,
            Some(json!({
                "from": format!("Shopper <{mask}>"),
                "to": ["support@shop.example"],
                "subject": "about my order",
                "body": "where is it",
            })),
        )
        .await;
    assert_eq!(status, 200, "{sent}");
    let jobs = queued(&kevy, sent["message_id"].as_str().expect("id"));
    assert_eq!(jobs.len(), 1);
    let (sender, raw) = &jobs[0];
    assert_eq!(sender, &mask, "the envelope sender is the mask");
    let head = headers(raw).to_ascii_lowercase();
    assert!(!head.contains("owner@"), "the account leaked:\n{head}");
    assert!(
        !head.contains("owner"),
        "the account's name leaked:\n{head}"
    );
    assert!(head.contains(&mask), "{head}");

    // turned off, it takes no mail; deleted, it stays taken
    let path = format!("/api/masked-emails/{mask}");
    let (status, _) = live
        .call(
            Method::PATCH,
            &path,
            &session,
            Some(json!({ "state": "disabled" })),
        )
        .await;
    assert_eq!(status, 200);
    assert!(!live.core.recipient_deliverable(&mask).await.expect("check"));
    let (status, _) = live.call(Method::DELETE, &path, &session, None).await;
    assert_eq!(status, 204);
    let (_, list) = live
        .call(Method::GET, "/api/masked-emails", &session, None)
        .await;
    assert_eq!(list, json!([]));
    let (status, _) = live
        .call(
            Method::PATCH,
            &path,
            &session,
            Some(json!({ "state": "enabled" })),
        )
        .await;
    assert_eq!(status, 409, "a deleted mask cannot come back");

    // JMAP makes and lists masks
    let (status, answer) = live
        .call(
            Method::POST,
            "/jmap",
            &session,
            Some(json!({
                "using": ["urn:ietf:params:jmap:core", "https://www.fastmail.com/dev/maskedemail"],
                "methodCalls": [
                    ["MaskedEmail/set", { "accountId": OWNER, "create": { "k1": { "forDomain": "news.example", "emailPrefix": "newsletter" } } }, "0"],
                    ["MaskedEmail/get", { "accountId": OWNER, "ids": null }, "1"]
                ]
            })),
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    let created = &answer["methodResponses"][0][1]["created"]["k1"];
    assert_eq!(created["forDomain"], "news.example", "{answer}");
    let made = created["email"].as_str().expect("email");
    assert!(
        !made.contains("newsletter"),
        "emailPrefix is ignored: {made}"
    );
    assert_eq!(
        made.split('.').nth(2).map(str::len),
        Some(6 + "@example".len())
    );
    let listed = &answer["methodResponses"][1][1]["list"];
    assert_eq!(listed[0]["email"], created["email"], "{answer}");

    // a key that may only manage masks
    let (status, key) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &session,
            Some(json!({ "name": "password manager", "scopes": ["masked_email"] })),
        )
        .await;
    assert_eq!(status, 200, "{key}");
    let key = key["secret"].as_str().expect("secret").to_string();
    let (status, _) = live
        .call(Method::GET, "/api/masked-emails", &key, None)
        .await;
    assert_eq!(status, 200);
    let (status, _) = live
        .call(Method::GET, "/api/conversations", &key, None)
        .await;
    assert_eq!(status, 403, "a mask key reads no mail");
    let (_, answer) = live
        .call(
            Method::POST,
            "/jmap",
            &key,
            Some(json!({
                "using": ["urn:ietf:params:jmap:core"],
                "methodCalls": [
                    ["Mailbox/get", { "accountId": OWNER }, "0"],
                    ["MaskedEmail/get", { "accountId": OWNER }, "1"]
                ]
            })),
        )
        .await;
    assert_eq!(answer["methodResponses"][0][0], "error", "{answer}");
    assert_eq!(
        answer["methodResponses"][0][1]["type"], "forbidden",
        "{answer}"
    );
    assert_eq!(
        answer["methodResponses"][1][0], "MaskedEmail/get",
        "{answer}"
    );

    bitwarden_makes_a_mask(&live, &key).await;
}

const MASKED_CAP: &str = "https://www.fastmail.com/dev/maskedemail";

/// The two requests Bitwarden's Fastmail forwarder sends, as written in
/// bitwarden/clients `libs/tools/generator/core/src/integration/fastmail.ts`:
/// the session read from `/jmap/session` (it does not follow the
/// `/.well-known/jmap` redirect) for the masked-email primary account, then
/// one `MaskedEmail/set` posted to `/jmap/api/`, read back from
/// `created["new-masked-email"].email`. The key is sent as a bearer token.
async fn bitwarden_makes_a_mask(live: &live::Live, key: &str) {
    let (status, session) = live.call(Method::GET, "/jmap/session", key, None).await;
    assert_eq!(status, 200, "{session}");
    let account = session["primaryAccounts"][MASKED_CAP]
        .as_str()
        .expect("a masked-email primary account")
        .to_string();
    assert_eq!(account, OWNER);

    let (status, answer) = live
        .call(
            Method::POST,
            "/jmap/api/",
            key,
            Some(json!({
                "using": [MASKED_CAP, "urn:ietf:params:jmap:core"],
                "methodCalls": [[
                    "MaskedEmail/set",
                    {
                        "accountId": account,
                        "create": {
                            "new-masked-email": {
                                "state": "enabled",
                                "description": "",
                                "forDomain": "accounts.example.org",
                                "emailPrefix": "bwprefix",
                            }
                        }
                    },
                    "0"
                ]]
            })),
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(
        answer["methodResponses"][0][0], "MaskedEmail/set",
        "{answer}"
    );
    let made = &answer["methodResponses"][0][1]["created"]["new-masked-email"];
    let email = made["email"].as_str().expect("created email");
    assert!(email.ends_with("@example.test"), "{answer}");
    assert!(
        !email.contains("bwprefix"),
        "emailPrefix is accepted and ignored: {email}"
    );
    assert_eq!(made["forDomain"], "accounts.example.org");
    assert_eq!(made["state"], "enabled");
    assert!(live.core.recipient_deliverable(email).await.expect("check"));
}
