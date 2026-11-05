//! A draft keeps its files: they are uploaded on their own, the draft names
//! them, and a send that names the draft carries them, ahead of the form's
//! own, byte for byte, and deletes the draft. A client that does not ask
//! for drafts with files is not shown them, since it would send one without
//! them. A refused send keeps the draft.
//!
//! Against a real kevy-server: the queue write after the copy is
//! pipelined, which the in-process kevy does not support.

mod live;

use base64::Engine as _;
use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const BOUNDARY: &str = "mailrs-draft-boundary";

fn part(name: &str, file: Option<(&str, &[u8])>, value: &str) -> Vec<u8> {
    let mut b = match file {
        Some((filename, _)) => format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; \
             filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes(),
        None => format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}"
        )
        .into_bytes(),
    };
    if let Some((_, bytes)) = file {
        b.extend(bytes);
    }
    b.extend(b"\r\n");
    b
}

async fn multipart(
    live: &live::Live,
    token: &str,
    path: &str,
    parts: Vec<Vec<u8>>,
) -> (u16, Value) {
    let mut body: Vec<u8> = parts.concat();
    body.extend(format!("--{BOUNDARY}--\r\n").as_bytes());
    let res = live
        .http
        .post(format!("{}{path}", live.base))
        .bearer_auth(token)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .await
        .expect("request");
    let status = res.status().as_u16();
    let text = res.text().await.expect("body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn upload(live: &live::Live, token: &str, name: &str, bytes: &[u8]) -> Value {
    let (status, got) = multipart(
        live,
        token,
        "/api/mail/draft-attachments",
        vec![part("file", Some((name, bytes)), "")],
    )
    .await;
    assert_eq!(status, 200, "{got}");
    got
}

async fn drafts(live: &live::Live, token: &str, query: &str) -> Vec<Value> {
    let (status, got) = live
        .call(
            Method::GET,
            &format!("/api/mail/drafts{query}"),
            token,
            None,
        )
        .await;
    assert_eq!(status, 200, "{got}");
    got.as_array().expect("array").clone()
}

fn sent(root: &std::path::Path) -> Vec<String> {
    let cur = root.join("example.test/alice/.Sent/cur");
    std::fs::read_dir(cur)
        .map(|d| {
            d.map(|e| String::from_utf8_lossy(&std::fs::read(e.unwrap().path()).unwrap()).into())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_sends_with_its_files_and_old_clients_never_see_it() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    let uploads = tempfile::tempdir().expect("uploads");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
        std::env::set_var("MAILRS_JMAP_UPLOAD_DIR", uploads.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    let second = upload(
        &live,
        &alice,
        "second.txt",
        b"the second file, from the draft",
    )
    .await;
    let first = upload(
        &live,
        &alice,
        "first.txt",
        b"the first file, from the draft",
    )
    .await;
    assert_eq!(first["size"], 30);

    let (status, saved) = live
        .call(
            Method::POST,
            "/api/mail/drafts",
            &alice,
            Some(json!({
                "to": "carol@partner.test", "subject": "files", "body": "attached",
                "attachments": [first, second],
            })),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    let id = saved["id"].as_i64().expect("id");

    // a client that did not ask is not shown it; one that asks is
    assert!(drafts(&live, &alice, "").await.is_empty());
    let listed = drafts(&live, &alice, "?with_attachments=true").await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["attachments"][1]["name"], "second.txt");

    // only the owner downloads a file
    let blob = first["blob_id"].as_str().unwrap();
    let path = format!("/api/mail/draft-attachments/{blob}?name=first.txt");
    let res = live
        .http
        .get(format!("{}{path}", live.base))
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        res.bytes().await.unwrap().as_ref(),
        b"the first file, from the draft"
    );
    let (status, _) = live.call(Method::GET, &path, &bob, None).await;
    assert_eq!(status, 404);

    // naming a file that is not an upload of one's own writes nothing
    let (status, got) = live
        .call(
            Method::POST,
            "/api/mail/drafts",
            &bob,
            Some(json!({ "subject": "x", "attachments": [first] })),
        )
        .await;
    assert_eq!(status, 422, "{got}");
    assert!(
        got["missing"][0].as_str().unwrap().contains("first.txt"),
        "{got}"
    );
    assert!(
        drafts(&live, &bob, "?with_attachments=true")
            .await
            .is_empty()
    );

    // the draft's files and the form's together over the limit: refused,
    // naming what to drop, and the draft kept
    const MIB: usize = 1024 * 1024;
    let big = upload(&live, &alice, "video.mov", &vec![b'v'; 20 * MIB]).await;
    let (status, saved) = live
        .call(
            Method::POST,
            "/api/mail/drafts",
            &alice,
            Some(json!({ "subject": "too much", "attachments": [big] })),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    let big_id = saved["id"].as_i64().expect("id");
    let (status, got) = multipart(
        &live,
        &alice,
        "/api/mail/send-multipart",
        vec![
            part("to", None, "carol@partner.test"),
            part("draft_id", None, &big_id.to_string()),
            part(
                "attachments",
                Some(("slides.pdf", &vec![b's'; 10 * MIB])),
                "",
            ),
        ],
    )
    .await;
    assert_eq!(status, 413, "{got}");
    assert_eq!(got["drop"], json!(["video.mov"]), "{got}");
    assert_eq!(
        drafts(&live, &alice, "?with_attachments=true").await.len(),
        2
    );
    assert!(sent(tmp.path()).is_empty());
    live.call(
        Method::DELETE,
        &format!("/api/mail/drafts/{big_id}"),
        &alice,
        None,
    )
    .await;

    let (status, got) = multipart(
        &live,
        &alice,
        "/api/mail/send-multipart",
        vec![
            part("to", None, "carol@partner.test"),
            part("subject", None, "files"),
            part("body", None, "attached"),
            part("draft_id", None, &id.to_string()),
            part(
                "attachments",
                Some(("added.txt", b"added in the composer")),
                "",
            ),
        ],
    )
    .await;
    assert_eq!(status, 200, "{got}");
    let copies = sent(tmp.path());
    assert_eq!(copies.len(), 1);
    let at = |bytes: &[u8]| {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        copies[0]
            .find(&b64)
            .unwrap_or_else(|| panic!("{b64} not in the message"))
    };
    let order = [
        at(b"the first file, from the draft"),
        at(b"the second file, from the draft"),
        at(b"added in the composer"),
    ];
    assert!(
        order.is_sorted(),
        "the draft's files go first, in its order: {order:?}"
    );
    assert!(
        drafts(&live, &alice, "?with_attachments=true")
            .await
            .is_empty(),
        "sent, so gone"
    );

    // sending it again finds no draft
    let (status, got) = multipart(
        &live,
        &alice,
        "/api/mail/send-multipart",
        vec![
            part("to", None, "carol@partner.test"),
            part("draft_id", None, &id.to_string()),
        ],
    )
    .await;
    assert_eq!(status, 404, "{got}");
    assert_eq!(sent(tmp.path()).len(), 1);
}
