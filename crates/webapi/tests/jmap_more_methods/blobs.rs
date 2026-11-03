//! Blobs an account may and may not name, and inline attachments.

use serde_json::{Value, json};

use super::{ALICE, BOB, import, live, message, one, upload};

/// An attachment by blobId is only what this account can reach. Another
/// account's upload, another account's message, a held message of one's
/// own and a blob that never existed are answered the same way, so the
/// answer says nothing about which of them a blob is.
pub(super) async fn blobs_not_yours(live: &live::Live, alice: &str, bob: &str) {
    let theirs_upload = upload(live, bob, BOB, b"bob's notes").await;
    let theirs_message = import(
        live,
        bob,
        BOB,
        &message("Bob private", "Friend <friend@partner.example>", "private"),
    )
    .await;
    let held = import(
        live,
        alice,
        ALICE,
        &message("Held one", "Shop <shop@partner.example>", "held"),
    )
    .await;
    live.mailbox
        .set_quarantined(ALICE, "Held-one@partner.example", true)
        .expect("hold");
    let never = format!("U{}", "0".repeat(64));
    let blob_of = |email_id: &str| format!("B{}", email_id.trim_start_matches('M'));
    let mut answers = Vec::new();
    for blob in [
        theirs_upload,
        never,
        blob_of(&theirs_message).replace('B', "B9999"),
        blob_of(&held),
        format!("{}-0", blob_of(&held)),
    ] {
        let r = one(
            live,
            alice,
            json!(["Email/set", { "create": { "d": {
                "mailboxIds": { "drafts": true }, "from": [{ "email": ALICE }],
                "textBody": [{ "partId": "1", "type": "text/plain" }],
                "bodyValues": { "1": { "value": "x" } },
                "attachments": [{ "blobId": blob, "type": "text/plain" }],
            } } }, "0"]),
        )
        .await;
        let e = r["notCreated"]["d"].clone();
        assert_eq!(e["type"], "blobNotFound", "{blob}: {r}");
        // the only difference is the id that was asked about
        let shape = e.to_string().replace(&blob, "<blob>");
        answers.push(shape);
        // and its download is the same 404 as one that never was
        let status = live
            .http
            .get(format!("{}/jmap/download/{ALICE}/{blob}/x", live.base))
            .bearer_auth(alice)
            .send()
            .await
            .expect("download")
            .status()
            .as_u16();
        assert_eq!(status, 404, "{blob}");
    }
    answers.dedup();
    assert_eq!(
        answers.len(),
        1,
        "the answers tell the blobs apart: {answers:?}"
    );
}

/// An attachment marked inline with a `cid` travels beside the HTML that
/// shows it, under that Content-ID, not as a file.
pub(super) async fn inline_image(live: &live::Live, alice: &str) {
    let png = upload(live, alice, ALICE, b"\x89PNG-logo").await;
    let r = one(
        live,
        alice,
        json!(["Email/set", { "create": { "d": {
            "mailboxIds": { "drafts": true }, "keywords": { "$draft": true },
            "from": [{ "email": ALICE }], "subject": "Logo",
            "htmlBody": [{ "partId": "h", "type": "text/html" }],
            "bodyValues": { "h": { "value": "<p><img src=\"cid:logo@x\"></p>" } },
            "attachments": [{ "blobId": png, "type": "image/png", "disposition": "inline", "cid": "logo@x" }],
        } } }, "0"]),
    )
    .await;
    let id = r["created"]["d"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_string();
    let got = one(
        live,
        alice,
        json!(["Email/get", { "ids": [id], "properties": ["attachments", "hasAttachment"] }, "0"]),
    )
    .await;
    let att = &got["list"][0]["attachments"][0];
    assert_eq!(att["cid"], "logo@x", "{got}");
    assert_eq!(att["disposition"], "inline", "{got}");
    assert_eq!(
        att["name"],
        Value::Null,
        "an inline image is not a named file: {got}"
    );
}
