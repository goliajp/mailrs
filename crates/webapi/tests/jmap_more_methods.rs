//! The JMAP methods beyond reading and filing, against the real router, a
//! real fastcore and a real kevy-server: the vacation reply, search
//! snippets, attachments on a new message, a held submission that is
//! listed, changed and cancelled, copying from another account, and the
//! same API over a WebSocket with push.

mod live;

#[path = "jmap_more_methods/blobs.rs"]
mod blobs;
#[path = "jmap_more_methods/creation.rs"]
mod creation;
#[path = "jmap_more_methods/filters.rs"]
mod filters;
#[path = "jmap_more_methods/sockets.rs"]
mod sockets;

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const ORIGIN: &str = "https://mail.example.test";
const USING: [&str; 4] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:submission",
    "urn:ietf:params:jmap:vacationresponse",
];

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

/// The first answer's arguments.
async fn one(live: &live::Live, token: &str, call: Value) -> Value {
    let r = jmap(live, token, json!([call])).await;
    r["methodResponses"][0][1].clone()
}

fn message(subject: &str, from: &str, body: &str) -> String {
    format!(
        "From: {from}\r\nTo: friend@partner.example\r\nSubject: {subject}\r\nMessage-ID: <{}@partner.example>\r\nDate: Sun, 01 Nov 2026 09:00:00 +0000\r\n\r\n{body}\r\n",
        subject.replace(' ', "-")
    )
}

async fn upload(live: &live::Live, token: &str, account: &str, bytes: &[u8]) -> String {
    let r: Value = live
        .http
        .post(format!("{}/jmap/upload/{account}/", live.base))
        .bearer_auth(token)
        .header("content-type", "application/octet-stream")
        .body(bytes.to_vec())
        .send()
        .await
        .expect("upload")
        .json()
        .await
        .expect("upload body");
    r["blobId"].as_str().expect("blobId").to_string()
}

/// Import `raw` into the inbox; its id.
async fn import(live: &live::Live, token: &str, account: &str, raw: &str) -> String {
    let blob = upload(live, token, account, raw.as_bytes()).await;
    let r = one(
        live,
        token,
        json!(["Email/import", { "emails": { "e": { "blobId": blob, "mailboxIds": { "inbox": true } } } }, "0"]),
    )
    .await;
    r["created"]["e"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string()
}

#[test]
fn the_other_jmap_methods_work_against_a_real_core() {
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
        std::env::set_var("MAILRS_WEB_ORIGINS", ORIGIN);
    }
    let live = live::start(&kevy).await;
    for who in [ALICE, BOB] {
        live.add_account(who, "pw").await;
    }
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    let (_, session) = live.call(Method::GET, "/jmap/session", &alice, None).await;
    assert!(
        session["capabilities"]["urn:ietf:params:jmap:vacationresponse"].is_object(),
        "{session}"
    );
    let caps = &session["accounts"][ALICE]["accountCapabilities"];
    assert!(caps["urn:ietf:params:jmap:submission"]["maxDelayedSend"].as_u64() > Some(0));

    vacation(&live, &alice).await;
    snippets(&live, &alice, &bob).await;
    submissions(&live, &alice, &bob).await;
    creation::draft_and_send_in_one_request(&live, &alice).await;
    filters::header_filters_look_in_their_header(&live, &alice).await;
    copy(&live, &alice).await;
    sockets::websocket(&live, &alice).await;
    sockets::limits_alike(&live, &alice).await;
    sockets::reconnect_catches_up(&live, &alice).await;
    blobs::blobs_not_yours(&live, &alice, &bob).await;
    blobs::inline_image(&live, &alice).await;
}

async fn vacation(live: &live::Live, alice: &str) {
    let got = one(
        live,
        alice,
        json!(["VacationResponse/get", { "ids": null }, "0"]),
    )
    .await;
    assert_eq!(got["list"][0]["id"], "singleton", "{got}");
    assert_eq!(got["list"][0]["isEnabled"], false);
    let set = one(
        live,
        alice,
        json!(["VacationResponse/set", { "update": { "singleton": {
            "isEnabled": true, "subject": "Away", "textBody": "Back on Monday",
            "toDate": "2026-12-01T00:00:00Z"
        } } }, "0"]),
    )
    .await;
    assert!(set["updated"].get("singleton").is_some(), "{set}");
    assert_ne!(set["oldState"], set["newState"]);
    // the web's settings read the same record
    let (_, web) = live
        .call(Method::GET, "/api/settings/vacation", alice, None)
        .await;
    assert_eq!(web["enabled"], true, "{web}");
    assert_eq!(web["body"], "Back on Monday");
    assert_eq!(web["end"], 1_796_083_200);
    let refused = one(
        live,
        alice,
        json!(["VacationResponse/set", {
            "update": { "singleton": { "htmlBody": "<p>x</p>" } },
            "create": { "c": {} },
            "destroy": ["singleton"]
        }, "0"]),
    )
    .await;
    assert_eq!(
        refused["notUpdated"]["singleton"]["type"], "invalidProperties",
        "{refused}"
    );
    assert_eq!(refused["notCreated"]["c"]["type"], "singleton");
    assert_eq!(refused["notDestroyed"]["singleton"]["type"], "singleton");
}

async fn snippets(live: &live::Live, alice: &str, bob: &str) {
    let id = import(
        live,
        alice,
        ALICE,
        &message(
            "Quarterly invoice",
            "Shop <shop@partner.example>",
            "Your invoice for <March> is attached.",
        ),
    )
    .await;
    let bobs = import(
        live,
        bob,
        BOB,
        &message("Bob invoice", "Shop <shop@partner.example>", "invoice"),
    )
    .await;
    let r = one(
        live,
        alice,
        json!(["SearchSnippet/get", { "filter": { "text": "invoice" }, "emailIds": [id, "M999999"] }, "0"]),
    )
    .await;
    assert_eq!(r["list"][0]["emailId"], id, "{r}");
    assert_eq!(r["list"][0]["subject"], "Quarterly <mark>invoice</mark>");
    let preview = r["list"][0]["preview"].as_str().expect("preview");
    assert!(
        preview.contains("<mark>invoice</mark>") && preview.contains("&lt;March&gt;"),
        "{preview}"
    );
    assert_eq!(r["notFound"], json!(["M999999"]));
    // another account's message is not found, even under its own id
    if bobs != id {
        let r = one(
            live,
            alice,
            json!(["SearchSnippet/get", { "filter": { "text": "invoice" }, "emailIds": [bobs] }, "0"]),
        )
        .await;
        assert_eq!(r["notFound"], json!([bobs]), "{r}");
    }
}

async fn identity(live: &live::Live, token: &str) -> String {
    let r = one(live, token, json!(["Identity/get", {}, "i"])).await;
    r["list"][0]["id"].as_str().expect("identity").to_string()
}

/// A draft from Alice carrying `blob` as an attachment; its id.
async fn draft(live: &live::Live, alice: &str, subject: &str, blob: Option<&str>) -> String {
    let attachments = match blob {
        Some(b) => json!([{ "blobId": b, "type": "application/pdf", "name": "plan.pdf" }]),
        None => json!([]),
    };
    let r = one(
        live,
        alice,
        json!(["Email/set", { "create": { "d": {
            "mailboxIds": { "drafts": true }, "keywords": { "$draft": true },
            "from": [{ "email": ALICE }], "to": [{ "email": "carol@partner.example" }],
            "subject": subject,
            "textBody": [{ "partId": "1", "type": "text/plain" }],
            "bodyValues": { "1": { "value": "see attached" } },
            "attachments": attachments,
        } } }, "0"]),
    )
    .await;
    r["created"]["d"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string()
}

async fn submissions(live: &live::Live, alice: &str, bob: &str) {
    // attachments by blobId: an upload of one's own, not another's
    let pdf = b"%PDF-1.4 the plan";
    let blob = upload(live, alice, ALICE, pdf).await;
    let id = draft(live, alice, "The plan", Some(&blob)).await;
    let got = one(
        live,
        alice,
        json!(["Email/get", { "ids": [id], "properties": ["attachments"] }, "0"]),
    )
    .await;
    let att = &got["list"][0]["attachments"][0];
    assert_eq!(att["name"], "plan.pdf", "{got}");
    assert_eq!(att["type"], "application/pdf");
    let part: Vec<u8> = live
        .http
        .get(format!(
            "{}/jmap/download/{ALICE}/{}/plan.pdf",
            live.base,
            att["blobId"].as_str().unwrap()
        ))
        .bearer_auth(alice)
        .send()
        .await
        .expect("download")
        .bytes()
        .await
        .expect("bytes")
        .to_vec();
    assert_eq!(part, pdf);
    let bobs_blob = upload(live, bob, BOB, b"bob's file").await;
    let r = one(
        live,
        alice,
        json!(["Email/set", { "create": { "d": {
            "mailboxIds": { "drafts": true }, "from": [{ "email": ALICE }],
            "textBody": [{ "partId": "1", "type": "text/plain" }],
            "bodyValues": { "1": { "value": "x" } },
            "attachments": [{ "blobId": bobs_blob, "type": "text/plain" }],
        } } }, "0"]),
    )
    .await;
    assert_eq!(r["notCreated"]["d"]["type"], "blobNotFound", "{r}");

    // held for an hour: pending, listed, then cancelled
    let identity = identity(live, alice).await;
    let before = one(
        live,
        alice,
        json!(["EmailSubmission/get", { "ids": [] }, "0"]),
    )
    .await;
    let s0 = before["state"].as_str().expect("state").to_string();
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "create": { "k": {
            "emailId": id, "identityId": identity,
            "envelope": {
                "mailFrom": { "email": ALICE, "parameters": { "HOLDFOR": "3600" } },
                "rcptTo": [{ "email": "carol@partner.example" }]
            }
        } } }, "0"]),
    )
    .await;
    assert_eq!(r["created"]["k"]["undoStatus"], "pending", "{r}");
    let sub = r["created"]["k"]["id"].as_str().expect("id").to_string();
    assert_eq!(r["oldState"], s0);
    let got = one(
        live,
        alice,
        json!(["EmailSubmission/get", { "ids": [sub] }, "0"]),
    )
    .await;
    let o = &got["list"][0];
    assert_eq!(o["undoStatus"], "pending", "{got}");
    assert_eq!(o["emailId"], id);
    assert_eq!(o["identityId"], identity);
    assert_eq!(
        o["deliveryStatus"]["carol@partner.example"]["delivered"],
        "queued"
    );
    let send_at = chrono::DateTime::parse_from_rfc3339(o["sendAt"].as_str().unwrap())
        .unwrap()
        .timestamp();
    let now = chrono::Utc::now().timestamp();
    assert!((now + 3500..=now + 3700).contains(&send_at), "{o}");
    let ch = one(
        live,
        alice,
        json!(["EmailSubmission/changes", { "sinceState": s0 }, "0"]),
    )
    .await;
    assert_eq!(ch["created"], json!([sub]), "{ch}");
    // another account does not see it
    let theirs = one(
        live,
        bob,
        json!(["EmailSubmission/get", { "ids": [sub] }, "0"]),
    )
    .await;
    assert_eq!(theirs["notFound"], json!([sub]), "{theirs}");
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "update": { (sub.clone()): { "undoStatus": "canceled" } } }, "0"]),
    )
    .await;
    assert!(r["updated"].get(&sub).is_some(), "{r}");
    let got = one(
        live,
        alice,
        json!(["EmailSubmission/get", { "ids": [sub] }, "0"]),
    )
    .await;
    assert_eq!(got["list"][0]["undoStatus"], "canceled", "{got}");
    let (_, sends) = live.call(Method::GET, "/api/mail/sends", alice, None).await;
    assert!(sends.to_string().contains("\"cancelled\""), "{sends}");
    // the cancel is a new fact beside the one that it was scheduled, on the
    // send and in the audit log; the send itself is still there
    let held = sends
        .as_array()
        .and_then(|l| l.iter().find(|s| s["subject"] == "The plan"))
        .expect("the held send is still listed");
    assert_eq!(held["status"], "cancelled");
    let send_id = held["send_id"].as_str().expect("send_id").to_string();
    let url = std::env::var("MAILRS_KEVY_URL").expect("kevy url");
    let (events, audit) = tokio::task::spawn_blocking(move || {
        use mailrs_core_sidestate::families::send as sendfam;
        let mut c = kevy_client::Connection::connect(&url).expect("kevy");
        let events = sendfam::events(&mut c, ALICE, &send_id).expect("events");
        let audit: Vec<(String, String)> = c
            .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, 50)
            .expect("audit")
            .iter()
            .filter_map(|b| serde_json::from_slice::<Value>(b).ok())
            .filter(|f| f["target"] == json!(send_id))
            .map(|f| {
                (
                    f["action"].as_str().unwrap_or("").to_string(),
                    f["actor"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        (events, audit)
    })
    .await
    .expect("read");
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["scheduled", "cancelled"], "{events:?}");
    assert!(events.iter().all(|e| e.by == ALICE), "{events:?}");
    assert!(events[0].send_at.is_some());
    let actions: Vec<&str> = audit.iter().map(|(a, _)| a.as_str()).collect();
    assert!(
        actions.contains(&"send.scheduled") && actions.contains(&"send.cancelled"),
        "{audit:?}"
    );

    // sent at once: final, and cannot be taken back
    let now_id = draft(live, alice, "Right now", None).await;
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "create": { "n": { "emailId": now_id, "identityId": identity } } }, "0"]),
    )
    .await;
    assert_eq!(r["created"]["n"]["undoStatus"], "final", "{r}");
    let sent = r["created"]["n"]["id"].as_str().unwrap().to_string();
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "update": { (sent.clone()): { "undoStatus": "canceled" } } }, "0"]),
    )
    .await;
    assert_eq!(r["notUpdated"][&sent]["type"], "cannotUnsend", "{r}");
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "create": { "x": {
            "emailId": now_id, "identityId": identity,
            "envelope": { "mailFrom": { "email": ALICE, "parameters": { "HOLDFOR": "99999999" } },
                          "rcptTo": [{ "email": "carol@partner.example" }] }
        } } }, "0"]),
    )
    .await;
    assert_eq!(
        r["notCreated"]["x"]["type"], "invalidProperties",
        "longer than maxDelayedSend: {r}"
    );
    // the same cap by date: 31 days from now
    let late = (chrono::Utc::now() + chrono::Duration::days(31))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let r = one(
        live,
        alice,
        json!(["EmailSubmission/set", { "create": { "y": {
            "emailId": now_id, "identityId": identity,
            "envelope": { "mailFrom": { "email": ALICE, "parameters": { "HOLDUNTIL": late } },
                          "rcptTo": [{ "email": "carol@partner.example" }] }
        } } }, "0"]),
    )
    .await;
    assert_eq!(r["notCreated"]["y"]["type"], "invalidProperties", "{r}");
    let (_, sends) = live.call(Method::GET, "/api/mail/sends", alice, None).await;
    let queued = sends.as_array().map_or(0, |l| l.len());
    assert_eq!(queued, 2, "a refused submission queued nothing: {sends}");
}

async fn copy(live: &live::Live, alice: &str) {
    let same = one(
        live,
        alice,
        json!(["Email/copy", { "fromAccountId": ALICE, "create": {} }, "0"]),
    )
    .await;
    assert_eq!(same["type"], "invalidArguments", "{same}");
    let other = one(
        live,
        alice,
        json!(["Email/copy", { "fromAccountId": BOB, "create": {} }, "0"]),
    )
    .await;
    assert_eq!(other["type"], "fromAccountNotFound", "{other}");
}
