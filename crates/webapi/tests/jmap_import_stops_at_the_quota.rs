//! `Email/import` stops at the quota as the mbox and IMAP imports do,
//! into the Sent folder as much as into the inbox, and says so with the
//! JMAP error for it (`overQuota`, RFC 8620 §5.3) rather than a server
//! failure. Against the real router, a real fastcore and a real kevy-server.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-jmap-import-stops-at-the-quota";
const ALICE: &str = "alice@example.test";
const USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"];

const OWN: &str = "From: Alice <alice@example.test>\r\nTo: carol@partner.test\r\nSubject: Lunch?\r\nMessage-ID: <own@oldmail.net>\r\nDate: Thu, 05 Jan 2023 09:00:00 +0000\r\n\r\nFriday?\r\n";
const RECEIVED: &str = "From: Carol <carol@partner.test>\r\nTo: alice@example.test\r\nSubject: Re: Lunch?\r\nMessage-ID: <in@partner.test>\r\nDate: Thu, 05 Jan 2023 10:00:00 +0000\r\n\r\nYes\r\n";

async fn upload(live: &live::Live, token: &str, bytes: &str) -> String {
    let res = live
        .http
        .post(format!("{}/jmap/upload/{ALICE}/", live.base))
        .bearer_auth(token)
        .header("content-type", "message/rfc822")
        .body(bytes.as_bytes().to_vec())
        .send()
        .await
        .expect("upload");
    let status = res.status();
    let text = res.text().await.expect("upload body");
    assert_eq!(status, 201, "upload: {text}");
    let r: Value = serde_json::from_str(&text).expect("upload json");
    r["blobId"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string()
}

async fn import_into(live: &live::Live, token: &str, blob: &str, mailbox: &str) -> Value {
    let call = json!(["Email/import", {
        "accountId": ALICE,
        "emails": { "e": { "blobId": blob, "mailboxIds": { mailbox: true } } }
    }, "0"]);
    let (status, body) = live
        .call(
            Method::POST,
            "/jmap/api/",
            token,
            Some(json!({ "using": USING, "methodCalls": [call] })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    body["methodResponses"][0][1].clone()
}

fn filed_in(root: &std::path::Path, folder: &str) -> usize {
    std::fs::read_dir(root.join("example.test/alice").join(folder).join("cur"))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_mailbox_takes_no_jmap_import() {
    let tmp = tempfile::tempdir().expect("tmp");
    let uploads = tempfile::tempdir().expect("uploads");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
        std::env::set_var("MAILRS_JMAP_UPLOAD_DIR", uploads.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;

    let mut net = kevy_client::Connection::connect(KEVY).expect("kevy");
    use mailrs_core_sidestate::keys::quota::{limit_key, used_key};
    net.set(limit_key(ALICE).as_bytes(), b"1000")
        .expect("limit");
    net.set(used_key(ALICE).as_bytes(), b"1000").expect("used");

    let own = upload(&live, &alice, OWN).await;
    let r = import_into(&live, &alice, &own, "sent").await;
    assert_eq!(
        r["notCreated"]["e"]["type"].as_str(),
        Some("overQuota"),
        "an own message was imported into Sent past the quota: {r}"
    );
    assert_eq!(
        filed_in(tmp.path(), ".Sent"),
        0,
        "a copy was filed past the quota"
    );

    let received = upload(&live, &alice, RECEIVED).await;
    let r = import_into(&live, &alice, &received, "inbox").await;
    assert_eq!(
        r["notCreated"]["e"]["type"].as_str(),
        Some("overQuota"),
        "{r}"
    );
}
