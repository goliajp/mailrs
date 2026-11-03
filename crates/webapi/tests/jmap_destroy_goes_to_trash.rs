//! `Email/set` destroy deletes the way the web client and IMAP do: a
//! conversation goes to the Trash first. Deleting one already there for
//! good is refused to an agent key, needs a recent confirmation from a
//! session, and is allowed to a password over HTTP Basic; each writes the
//! audit row every way of deleting for good writes. Against the real
//! router, a real fastcore and a real kevy-server.

mod live;

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"];

fn calls(call: Value) -> Value {
    json!({ "using": USING, "methodCalls": [call] })
}

/// The first answer's arguments, sent with `bearer`.
async fn one(live: &live::Live, bearer: &str, call: Value) -> Value {
    let (status, body) = live
        .call(Method::POST, "/jmap/api/", bearer, Some(calls(call)))
        .await;
    assert_eq!(status, 200, "{body}");
    body["methodResponses"][0][1].clone()
}

/// The same, with a password over HTTP Basic, as a mail app sends it.
async fn one_basic(live: &live::Live, call: Value) -> Value {
    let body: Value = live
        .http
        .post(format!("{}/jmap/api/", live.base))
        .basic_auth(ALICE, Some("pw"))
        .header("x-forwarded-proto", "https")
        .json(&calls(call))
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("body");
    body["methodResponses"][0][1].clone()
}

/// Import a letter into the inbox; its id.
async fn import(live: &live::Live, token: &str, subject: &str) -> String {
    let raw = format!(
        "From: Shop <shop@partner.example>\r\nTo: {ALICE}\r\nSubject: {subject}\r\n\
         Message-ID: <{subject}@partner.example>\r\nDate: Sun, 01 Nov 2026 09:00:00 +0000\r\n\r\nhi\r\n"
    );
    let up: Value = live
        .http
        .post(format!("{}/jmap/upload/{ALICE}/", live.base))
        .bearer_auth(token)
        .header("content-type", "application/octet-stream")
        .body(raw.into_bytes())
        .send()
        .await
        .expect("upload")
        .json()
        .await
        .expect("upload body");
    let r = one(
        live,
        token,
        json!(["Email/import", { "emails": { "e": { "blobId": up["blobId"], "mailboxIds": { "inbox": true } } } }, "0"]),
    )
    .await;
    r["created"]["e"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string()
}

fn destroy(id: &str) -> Value {
    json!(["Email/set", { "destroy": [id] }, "0"])
}

/// The mailboxes `id` is in; `None` once it is gone.
async fn mailboxes(live: &live::Live, token: &str, id: &str) -> Option<Value> {
    let got = one(
        live,
        token,
        json!(["Email/get", { "ids": [id], "properties": ["mailboxIds"] }, "0"]),
    )
    .await;
    got["list"].get(0).map(|e| e["mailboxIds"].clone())
}

/// Audit rows for mail deleted for good, newest first.
fn deletions(kevy: &str) -> Vec<Value> {
    let mut c = kevy_client::Connection::connect(kevy).expect("kevy");
    c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit log")
        .into_iter()
        .filter_map(|r| serde_json::from_slice::<Value>(&r).ok())
        .filter(|r| r["action"] == "mail.deleted_forever")
        .collect()
}

#[test]
fn destroy_moves_to_the_trash_and_deletes_for_good_only_when_allowed() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(async {
        tokio::time::timeout(Duration::from_secs(300), scenario())
            .await
            .expect("the scenario did not finish within 5 minutes")
    }));
    rt.shutdown_background();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn scenario() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let maildir = tempfile::tempdir().expect("maildir");
    let uploads = tempfile::tempdir().expect("uploads");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_HOSTNAME", "mail.example.test");
        std::env::set_var("MAILRS_MAILDIR", maildir.path());
        std::env::set_var("MAILRS_JMAP_UPLOAD_DIR", uploads.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    let session = live.login(ALICE, "pw").await;
    live.confirm(&session, "pw").await;
    let (_, made) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &session,
            Some(json!({ "name": "client", "scopes": ["jmap"] })),
        )
        .await;
    let key = made["secret"].as_str().expect("secret").to_string();
    // a fresh session, never confirmed
    let unconfirmed = live.login(ALICE, "pw").await;

    let first = import(&live, &session, "first").await;
    let second = import(&live, &session, "second").await;

    // out of the Trash, destroy moves it there, whoever asks
    let r = one(&live, &key, destroy(&first)).await;
    assert_eq!(r["destroyed"], json!([first]), "{r}");
    let boxes = mailboxes(&live, &session, &first)
        .await
        .expect("still there");
    assert_eq!(boxes, json!({ "trash": true }), "moved to the Trash");
    assert!(deletions(&kevy).is_empty(), "nothing was deleted for good");

    // in the Trash: not by an agent key, not by a session that did not confirm
    let r = one(&live, &key, destroy(&first)).await;
    assert_eq!(r["notDestroyed"][&first]["type"], "forbidden", "{r}");
    let r = one(&live, &unconfirmed, destroy(&first)).await;
    assert_eq!(r["notDestroyed"][&first]["type"], "forbidden", "{r}");
    assert!(
        r["notDestroyed"][&first]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("confirm_required"),
        "{r}"
    );
    assert!(mailboxes(&live, &session, &first).await.is_some());

    // a password over Basic deletes it for good, as an IMAP client may
    let r = one_basic(&live, destroy(&first)).await;
    assert_eq!(r["destroyed"], json!([first]), "{r}");
    assert!(mailboxes(&live, &session, &first).await.is_none(), "gone");

    // a confirmed session too
    one(&live, &session, destroy(&second)).await;
    let r = one(&live, &session, destroy(&second)).await;
    assert_eq!(r["destroyed"], json!([second]), "{r}");

    let rows = deletions(&kevy);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0]["actor"], ALICE);
    assert_eq!(rows[0]["detail"], "messages=1 via=jmap credential=session");
    assert_eq!(
        rows[1]["detail"],
        "messages=1 via=jmap credential=password_or_app_password"
    );
    assert_eq!(rows[1]["target"].as_str().map(str::len), Some(8));
}
