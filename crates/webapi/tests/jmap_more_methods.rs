//! The JMAP methods beyond reading and filing, against the real router, a
//! real fastcore and a real kevy-server: the vacation reply, search
//! snippets, attachments on a new message, a held submission that is
//! listed, changed and cancelled, copying from another account, and the
//! same API over a WebSocket with push.

mod live;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{
    Error as WsError, Message, client::IntoClientRequest, http::HeaderValue,
};

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
    copy(&live, &alice).await;
    websocket(&live, &alice).await;
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

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// An upgrade to `/jmap/ws/`: the socket, or the status it was refused with.
async fn upgrade(
    live: &live::Live,
    auth: (&'static str, &str),
    origin: Option<&str>,
    jmap: bool,
) -> Result<Socket, u16> {
    let url = format!("{}/jmap/ws/", live.base.replacen("http", "ws", 1));
    let mut req = url.into_client_request().expect("request");
    let h = req.headers_mut();
    h.insert(auth.0, HeaderValue::from_str(auth.1).expect("auth"));
    match origin {
        Some(o) => {
            h.insert("origin", HeaderValue::from_str(o).expect("origin"));
        }
        None => {
            h.remove("origin");
        }
    }
    if jmap {
        h.insert("sec-websocket-protocol", HeaderValue::from_static("jmap"));
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((socket, resp)) => {
            assert_eq!(resp.headers()["sec-websocket-protocol"], "jmap");
            Ok(socket)
        }
        Err(WsError::Http(resp)) => Err(resp.status().as_u16()),
        Err(e) => panic!("upgrade failed without a status: {e}"),
    }
}

/// The next JSON text frame, within `secs`.
async fn next_json(socket: &mut Socket, secs: u64) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, socket.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).ok(),
            Ok(Some(Ok(_))) => continue,
            _ => return None,
        }
    }
}

async fn websocket(live: &live::Live, alice: &str) {
    let bearer = format!("Bearer {alice}");
    let cookie = format!("mailrs_session={alice}");
    assert_eq!(
        upgrade(live, ("authorization", &bearer), None, false)
            .await
            .err(),
        Some(400),
        "the jmap subprotocol is required"
    );
    assert_eq!(
        upgrade(
            live,
            ("cookie", &cookie),
            Some("https://evil.example"),
            true
        )
        .await
        .err(),
        Some(403),
        "a cookie upgrade from another site"
    );
    assert!(
        upgrade(live, ("cookie", &cookie), Some(ORIGIN), true)
            .await
            .is_ok()
    );
    let Ok(mut ws) = upgrade(live, ("authorization", &bearer), None, true).await else {
        panic!("a bearer upgrade is refused");
    };
    let send = |v: Value| Message::Text(v.to_string().into());
    ws.send(send(json!({
        "@type": "Request", "id": "r1", "using": USING,
        "methodCalls": [["Core/echo", { "hello": 1 }, "0"]]
    })))
    .await
    .expect("send");
    let r = next_json(&mut ws, 10).await.expect("a response");
    assert_eq!(r["@type"], "Response", "{r}");
    assert_eq!(r["requestId"], "r1");
    assert_eq!(r["methodResponses"][0][1]["hello"], 1);
    ws.send(send(json!({ "@type": "Nonsense" })))
        .await
        .expect("send");
    let r = next_json(&mut ws, 10).await.expect("an error");
    assert_eq!(r["@type"], "RequestError", "{r}");

    // push: a change made over HTTP arrives on the socket
    ws.send(send(
        json!({ "@type": "WebSocketPushEnable", "dataTypes": null }),
    ))
    .await
    .expect("send");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let id = import(
        live,
        alice,
        ALICE,
        &message(
            "Pushed over the socket",
            "Friend <friend@partner.example>",
            "hi",
        ),
    )
    .await;
    let mut seen = None;
    while let Some(v) = next_json(&mut ws, 20).await {
        if v["@type"] == "StateChange" {
            seen = Some(v);
            break;
        }
    }
    let v = seen.expect("a StateChange within 20 s");
    assert!(v["changed"][ALICE]["Email"].is_string(), "{v} after {id}");
    assert_eq!(v["pushState"], v["changed"][ALICE]["Email"]);
}
