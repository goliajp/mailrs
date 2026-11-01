//! A JMAP client library (jmap-client, the one Stalwart publishes) against
//! the real router and a real fastcore: what a mail client does from
//! signing in to sending, and what the server must refuse.

mod live;

use std::time::Duration;

use futures_util::StreamExt;
use jmap_client::client::{Client, Credentials};
use jmap_client::mailbox::Role;
use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const ADMIN: &str = "admin@example.test";
const USING: [&str; 3] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:submission",
];

fn message(subject: &str, from: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: friend@partner.example\r\nSubject: {subject}\r\nMessage-ID: <{}@partner.example>\r\nDate: Sun, 01 Nov 2026 09:00:00 +0000\r\n\r\nhello {subject}\r\n",
        subject.replace(' ', "-")
    )
    .into_bytes()
}

/// One request, as the client's own calls.
async fn jmap(live: &live::Live, token: &str, calls: Value) -> Value {
    let (status, body) = live
        .call(
            Method::POST,
            "/jmap/api/",
            token,
            Some(json!({ "using": USING, "methodCalls": calls })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    body
}

async fn state_of(live: &live::Live, token: &str) -> String {
    let r = jmap(live, token, json!([["Email/get", { "ids": [] }, "s"]])).await;
    r["methodResponses"][0][1]["state"]
        .as_str()
        .expect("state")
        .to_string()
}

// Push starts webapi's feed consumers, blocking loops that end only with
// the process; a runtime dropped normally, as `#[tokio::test]` does and as
// a panic unwinding through `block_on` does, waits for them and never
// returns. So the runtime is let go in the background, and the scenario has
// a deadline that names it.
#[test]
fn a_jmap_client_reads_files_and_sends() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let outcome = rt.block_on(rt.spawn(async {
        tokio::time::timeout(Duration::from_secs(300), scenario())
            .await
            .expect("the JMAP client scenario did not finish within 5 minutes")
    }));
    rt.shutdown_background();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn scenario() {
    // a real kevy-server: the outbound queue's pipelined writes run there
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
    // SAFETY: as above; read on each session request
    unsafe { std::env::set_var("MAILRS_PUBLIC_BASE_URL", &live.base) };
    for who in [ALICE, BOB, ADMIN] {
        live.add_account(who, "pw").await;
    }
    live.make_super(ADMIN);
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    // the client takes plain http on loopback: it uses the session's URLs
    let client = Client::new()
        .credentials(Credentials::bearer(alice.clone()))
        .follow_redirects(["127.0.0.1"])
        .connect(&live.base)
        .await
        .expect("session");
    let inbox = client
        .mailbox_get("inbox", None::<Vec<_>>)
        .await
        .expect("get")
        .expect("an inbox");
    assert_eq!(inbox.role(), Role::Inbox);
    assert_eq!(inbox.total_emails(), 0);

    let s0 = state_of(&live, &alice).await;
    // upload, then import
    let raw = message("first note", "Friend <friend@partner.example>");
    let email = client
        .email_import(raw.clone(), ["inbox"], None::<Vec<String>>, None)
        .await
        .expect("import");
    let id = email.id().expect("id").to_string();
    let got = client
        .email_get(&id, None::<Vec<_>>)
        .await
        .expect("get")
        .expect("the email");
    assert_eq!(got.mailbox_ids(), ["inbox"]);
    assert!(got.keywords().is_empty(), "imported unread");
    let thread_id = got.thread_id().expect("thread").to_string();
    let changes = client.email_changes(&s0, None).await.expect("changes");
    assert_eq!(changes.created(), std::slice::from_ref(&id));
    assert!(changes.updated().is_empty() && changes.destroyed().is_empty());
    let inbox = client
        .mailbox_get("inbox", None::<Vec<_>>)
        .await
        .expect("get")
        .expect("inbox");
    assert_eq!((inbox.total_emails(), inbox.unread_emails()), (1, 1));

    // the whole message, as an attachment that cannot run in a page
    let blob = got.blob_id().expect("blob").to_string();
    let bytes = client.download(&blob).await.expect("download");
    assert!(String::from_utf8_lossy(&bytes).contains("hello first note"));
    let r = live
        .http
        .get(format!(
            "{}/jmap/download/{ALICE}/{blob}/m.html?type=text/html",
            live.base
        ))
        .bearer_auth(&alice)
        .send()
        .await
        .expect("download");
    assert_eq!(r.status(), 200);
    let h = r.headers();
    assert!(
        h["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(
        h.get_all("content-security-policy")
            .iter()
            .any(|v| v == "sandbox")
    );
    // another account's blob, or another account named, is not found
    for (token, account) in [(&bob, BOB), (&alice, BOB)] {
        let r = live
            .http
            .get(format!(
                "{}/jmap/download/{account}/{blob}/m.eml",
                live.base
            ))
            .bearer_auth(token)
            .send()
            .await
            .expect("download");
        assert_eq!(r.status(), 404, "{account}");
    }

    // a keyword, then a move: the id stays, the change is seen
    let s1 = state_of(&live, &alice).await;
    client
        .email_set_keyword(&id, "$seen", true)
        .await
        .expect("seen");
    let changes = client.email_changes(&s1, None).await.expect("changes");
    assert_eq!(changes.updated(), std::slice::from_ref(&id));
    client
        .email_set_mailboxes(&id, ["archive"])
        .await
        .expect("archive");
    let got = client
        .email_get(&id, None::<Vec<_>>)
        .await
        .expect("get")
        .expect("same id");
    assert_eq!(got.mailbox_ids(), ["archive"]);
    assert_eq!(got.keywords(), ["$seen"]);
    let thread = client
        .thread_get(&thread_id)
        .await
        .expect("thread")
        .expect("a thread");
    assert_eq!(thread.email_ids(), std::slice::from_ref(&id));
    let r = jmap(
        &live,
        &alice,
        json!([["Mailbox/changes", { "sinceState": s1 }, "m"]]),
    )
    .await;
    let updated = r["methodResponses"][0][1]["updated"].to_string();
    assert!(
        updated.contains("inbox") && updated.contains("archive"),
        "{r}"
    );

    // a state that was never handed out cannot be compared against
    let r = jmap(
        &live,
        &alice,
        json!([["Email/changes", { "sinceState": "0000000000000000" }, "c"]]),
    )
    .await;
    assert_eq!(
        r["methodResponses"][0][1]["type"], "cannotCalculateChanges",
        "{r}"
    );

    // push: a change made elsewhere arrives as a StateChange
    let mut events = client
        .event_source(None::<Vec<jmap_client::DataType>>, true, None, None)
        .await
        .expect("event source");
    tokio::time::sleep(Duration::from_millis(500)).await;
    client
        .email_set_keyword(&id, "$flagged", true)
        .await
        .expect("flag");
    let pushed = tokio::time::timeout(Duration::from_secs(20), events.next()).await;
    assert!(
        matches!(pushed, Ok(Some(Ok(_)))),
        "a StateChange within 20 s"
    );

    // aerc's send: import into Drafts, submit, move it to Sent on success
    let ids = jmap(&live, &alice, json!([["Identity/get", {}, "i"]])).await;
    let identity = ids["methodResponses"][0][1]["list"][0]["id"]
        .as_str()
        .expect("identity")
        .to_string();
    let draft = client
        .email_import(
            message("a reply", ALICE),
            ["drafts"],
            Some(["$draft"]),
            None,
        )
        .await
        .expect("draft");
    let draft_id = draft.id().expect("id").to_string();
    let got = client
        .email_get(&draft_id, None::<Vec<_>>)
        .await
        .expect("get")
        .expect("draft");
    assert_eq!(got.mailbox_ids(), ["drafts"]);
    let sent = jmap(
        &live,
        &alice,
        json!([["EmailSubmission/set", {
            "create": { "k1": { "emailId": draft_id, "identityId": identity } },
            "onSuccessUpdateEmail": { "#k1": {
                "keywords/$draft": null, "mailboxIds/sent": true, "mailboxIds/drafts": null
            } }
        }, "0"]]),
    )
    .await;
    let r = &sent["methodResponses"];
    assert_eq!(r[0][0], "EmailSubmission/set", "{sent}");
    assert!(r[0][1]["created"]["k1"]["id"].is_string(), "{sent}");
    assert_eq!(r[1][0], "Email/set", "{sent}");
    assert!(r[1][1]["updated"].get(&draft_id).is_some(), "{sent}");
    let got = client
        .email_get(&draft_id, None::<Vec<_>>)
        .await
        .expect("get")
        .expect("sent");
    assert_eq!(got.mailbox_ids(), ["sent"]);
    // queued: the send is recorded where the web client lists sends
    let (status, sends) = live
        .call(Method::GET, "/api/mail/sends", &alice, None)
        .await;
    assert_eq!(status, 200);
    assert!(sends.to_string().contains("a reply"), "{sends}");
    // a From that is not the identity is refused
    let forged = client
        .email_import(
            message("forged", "boss@elsewhere.example"),
            ["drafts"],
            Some(["$draft"]),
            None,
        )
        .await
        .expect("draft");
    let refused = jmap(
        &live,
        &alice,
        json!([["EmailSubmission/set", {
            "create": { "k2": { "emailId": forged.id().unwrap(), "identityId": identity } }
        }, "0"]]),
    )
    .await;
    assert_eq!(
        refused["methodResponses"][0][1]["notCreated"]["k2"]["type"], "forbiddenFrom",
        "{refused}"
    );

    limits_are_enforced(&live, &alice).await;
    basic_logins(&live).await;
}

async fn limits_are_enforced(live: &live::Live, alice: &str) {
    let calls: Vec<Value> = (0..17)
        .map(|n| json!(["Core/echo", {}, n.to_string()]))
        .collect();
    let (status, body) = live
        .call(
            Method::POST,
            "/jmap/api/",
            alice,
            Some(json!({ "using": USING, "methodCalls": calls })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(body["limit"], "maxCallsInRequest", "{body}");
    let (status, body) = live
        .call(
            Method::POST,
            "/jmap/api/",
            alice,
            Some(json!({ "using": ["urn:example:nothing"], "methodCalls": [] })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(body["type"], "urn:ietf:params:jmap:error:unknownCapability");
}

async fn basic(live: &live::Live, user: &str, password: &str, https: bool) -> u16 {
    let mut req = live
        .http
        .post(format!("{}/jmap/api/", live.base))
        .basic_auth(user, Some(password))
        .json(
            &json!({ "using": USING, "methodCalls": [["Mailbox/get", { "ids": ["inbox"] }, "0"]] }),
        );
    if https {
        req = req.header("x-forwarded-proto", "https");
    }
    req.send().await.expect("request").status().as_u16()
}

/// A mail client signs in with an app password over HTTP Basic, only over
/// HTTPS, and only with an app password once the account requires one.
async fn basic_logins(live: &live::Live) {
    assert_eq!(
        basic(live, BOB, "pw", false).await,
        403,
        "not over plain http"
    );
    assert_eq!(basic(live, BOB, "pw", true).await, 200);
    assert_eq!(basic(live, BOB, "wrong", true).await, 401);
    let bob = live.login(BOB, "pw").await;
    live.confirm(&bob, "pw").await;
    let (status, made) = live
        .call(
            Method::POST,
            "/api/auth/app-passwords",
            &bob,
            Some(json!({ "name": "aerc" })),
        )
        .await;
    assert_eq!(status, 201, "{made}");
    let app = made["password"].as_str().expect("password").to_string();
    let admin = live.login(ADMIN, "pw").await;
    let (status, _) = live
        .call(
            Method::PUT,
            "/api/admin/accounts/bob%40example.test/protocol-password",
            &admin,
            Some(json!({ "policy": "app-only" })),
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(
        basic(live, BOB, "pw", true).await,
        401,
        "the account password is refused"
    );
    assert_eq!(
        basic(live, BOB, &app, true).await,
        200,
        "the app password works"
    );
}
