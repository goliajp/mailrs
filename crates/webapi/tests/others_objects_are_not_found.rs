//! An identifier names an object only inside its owner's account. Asking
//! for somebody else's is answered as if it did not exist.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-others-objects-are-not-found";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test]
async fn another_accounts_objects_are_not_found() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    // alice first: the old JMAP lookup answered from the first account
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.confirm(&alice, "pw").await;
    let bob = live.login(BOB, "pw").await;
    live.confirm(&bob, "pw").await;

    jmap_mailboxes_are_the_callers(&live, &alice, &bob).await;
    webhooks_are_deleted_only_by_their_owner(&live, &alice, &bob).await;
    fraud_verdicts_are_read_by_recipients_only(&live, &alice, &bob).await;
    only_a_participant_files_a_thread(&live, &alice, &bob).await;
    a_stranger_learns_nothing_from_flagging(&live, &alice, &bob).await;
    scheduled_sends_are_moved_by_their_sender_only(&live, &alice, &bob).await;
    webhooks_over_mcp_stay_in_the_callers_account(&live, &bob).await;
    inline_uploads_cannot_carry_script(&live, &alice).await;
    oauth_client_registration_is_closed(&live).await;
}

async fn fraud_verdicts_are_read_by_recipients_only(live: &live::Live, alice: &str, bob: &str) {
    let mid = "verdict@partner.example";
    live.mailbox
        .upsert_user_message(
            ALICE,
            "t-alice",
            mid,
            1_786_000_000,
            br#"{"message_id":"verdict@partner.example","sender":"x@partner.example"}"#,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: "f.host",
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("message");
    live.mailbox
        .set_fraud_verdict(mid, r#"{"score":9.0}"#)
        .expect("verdict");
    let path = format!("/api/messages/{mid}/fraud-verdict");
    let (status, body) = live.call(Method::GET, &path, alice, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["verdict"]["score"], 9.0);
    let (status, _) = live.call(Method::GET, &path, bob, None).await;
    assert_eq!(status, 404, "bob reading the verdict on alice's mail");
}

/// Filing writes the shared thread row that every owner's row is rebuilt
/// from, so a stranger must not be able to do it.
async fn only_a_participant_files_a_thread(live: &live::Live, alice: &str, bob: &str) {
    let (status, _) = live
        .call(
            Method::POST,
            "/api/conversations/t-alice/mark-junk",
            bob,
            None,
        )
        .await;
    assert_eq!(status, 404, "bob filing alice's thread");
    let row = live
        .mailbox
        .get_thread_for_user(ALICE, "t-alice")
        .expect("row")
        .expect("alice's thread");
    assert_ne!(row.category, "spam", "alice's thread was moved by bob");
    let (status, _) = live
        .call(
            Method::POST,
            "/api/conversations/t-alice/mark-junk",
            alice,
            None,
        )
        .await;
    assert!(
        (200..300).contains(&status),
        "alice filing her own: {status}"
    );
}

/// Starring, pinning, archiving, marking read or unread and releasing
/// somebody else's thread answer exactly as for a thread that does not
/// exist, so the answer cannot be used to find out which ids exist.
async fn a_stranger_learns_nothing_from_flagging(live: &live::Live, alice: &str, bob: &str) {
    let verbs = [
        "star",
        "unstar",
        "pin",
        "unpin",
        "archive",
        "unarchive",
        "unread",
        "read",
    ];
    for verb in verbs {
        for thread in ["t-alice", "no-such-thread"] {
            let path = format!("/api/conversations/{thread}/{verb}");
            let (status, _) = live.call(Method::POST, &path, bob, None).await;
            assert_eq!(status, 404, "bob: {verb} {thread}");
        }
    }
    for thread in ["t-alice", "no-such-thread"] {
        let path = format!("/api/quarantine/{thread}/release");
        let (status, _) = live.call(Method::POST, &path, bob, None).await;
        assert_eq!(status, 404, "bob: release {thread}");
        let path = format!("/api/conversations/{thread}");
        let (status, _) = live.call(Method::DELETE, &path, bob, None).await;
        assert_eq!(status, 404, "bob: delete {thread}");
    }
    let row = live
        .mailbox
        .get_thread_for_user(ALICE, "t-alice")
        .expect("row")
        .expect("alice's thread");
    assert!(
        !row.starred && !row.pinned && !row.archived,
        "bob changed alice's thread"
    );
    for verb in verbs {
        let path = format!("/api/conversations/t-alice/{verb}");
        let (status, _) = live.call(Method::POST, &path, alice, None).await;
        assert!(
            (200..300).contains(&status),
            "alice: {verb} her own: {status}"
        );
    }
}

async fn scheduled_sends_are_moved_by_their_sender_only(live: &live::Live, alice: &str, bob: &str) {
    // written by hand: the in-process store has no pipelines, and a
    // cancel's WATCH/MULTI path is tested against a kevy-server in
    // core-sidestate (`another_senders_jobs_are_left_alone`)
    use mailrs_core_sidestate::families::outbound::{SCHEDULED_IDX, job_key};
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let id: i64 = 9_001;
    let blob = format!(r#"{{"sender":"{ALICE}","recipient":"someone@partner.example"}}"#);
    kv.hset(
        job_key(id).as_bytes(),
        &[
            (b"state" as &[u8], b"pending" as &[u8]),
            (b"blob", blob.as_bytes()),
        ],
    )
    .expect("job");
    kv.zadd(SCHEDULED_IDX, &[(4e9, id.to_string().as_bytes())])
        .expect("scheduled");
    let later = serde_json::json!({ "scheduled_at": 4_100_000_000i64 });
    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/scheduled/{id}/reschedule"),
            bob,
            Some(later.clone()),
        )
        .await;
    assert_eq!(status, 404, "bob moving alice's send");
    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/scheduled/{id}/cancel"),
            bob,
            None,
        )
        .await;
    assert_eq!(status, 404, "bob cancelling alice's send");
    let (status, _) = live
        .call(
            Method::POST,
            &format!("/api/scheduled/{id}/reschedule"),
            alice,
            Some(later),
        )
        .await;
    assert_eq!(status, 204, "alice moving her own");
    assert_eq!(
        kv.zscore(SCHEDULED_IDX, id.to_string().as_bytes())
            .expect("score"),
        Some(4.1e9),
        "moved to alice's new time"
    );
}

/// MCP reads and writes one's own webhooks in one's own account; anyone
/// else's needs an administrator.
async fn webhooks_over_mcp_stay_in_the_callers_account(live: &live::Live, bob: &str) {
    let mut mcp = live.mcp(bob).await;
    let mine = mcp
        .tool("list_webhooks", serde_json::json!({ "address": BOB }))
        .await;
    assert!(mine.is_ok(), "bob listing his own: {mine:?}");
    let theirs = mcp
        .tool("list_webhooks", serde_json::json!({ "address": ALICE }))
        .await;
    assert!(theirs.is_err(), "bob listed alice's webhooks: {theirs:?}");
    let made = mcp
        .tool(
            "create_webhook",
            serde_json::json!({ "account_address": ALICE, "url": "https://x.example", "event_type": "new_mail" }),
        )
        .await;
    assert!(
        made.is_err(),
        "bob created a webhook on alice's account: {made:?}"
    );
}

/// An uploaded inline image is served from this origin to whoever opens
/// the link, so it can only be an image a browser draws without script.
async fn inline_uploads_cannot_carry_script(live: &live::Live, alice: &str) {
    let upload = |ct: &'static str, body: &'static [u8]| async move {
        const BOUNDARY: &str = "inline-test-boundary";
        let mut form = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x\"\r\nContent-Type: {ct}\r\n\r\n"
        )
        .into_bytes();
        form.extend_from_slice(body);
        form.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        live.http
            .post(format!("{}/api/mail/inline-upload", live.base))
            .bearer_auth(alice)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(form)
            .send()
            .await
            .expect("upload")
    };
    let svg = upload("image/svg+xml", b"<svg onload='alert(1)'/>").await;
    assert_eq!(svg.status(), 415);
    let html = upload("text/html", b"<script>alert(1)</script>").await;
    assert_eq!(html.status(), 415);
    let png = upload("image/png", b"\x89PNG").await;
    assert_eq!(png.status(), 200);
    let url = png.json::<serde_json::Value>().await.expect("json")["url"]
        .as_str()
        .expect("url")
        .to_string();
    let res = live
        .http
        .get(format!("{}{url}", live.base))
        .bearer_auth(alice)
        .send()
        .await
        .expect("get");
    assert_eq!(res.headers()["content-type"], "image/png");
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert_eq!(res.headers()["content-security-policy"], "sandbox");
}

async fn oauth_client_registration_is_closed(live: &live::Live) {
    const ADMIN: &str = "admin@example.test";
    live.add_account(ADMIN, "pw").await;
    live.grant(ADMIN, "admin.accounts").await;
    // registering an OAuth client is admin.oauth_clients
    live.grant(ADMIN, "admin.oauth_clients").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;
    let (status, body) = live
        .call(
            Method::POST,
            "/api/admin/oauth-clients",
            &admin,
            Some(serde_json::json!({ "name": "x", "redirect_uri": "https://x.example/cb" })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "oauth_client_registration_closed");
}

/// Mailbox ids are the same small numbers in every account, so a JMAP
/// mailbox id must be read in the caller's account.
async fn jmap_mailboxes_are_the_callers(live: &live::Live, alice: &str, bob: &str) {
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "for alice only",
            senders_csv: "Friend <friend@partner.example>",
            subject: "for alice only",
            thread_id: "t-alice",
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let unread = |token: String| async move {
        let req = serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [["Mailbox/get", { "ids": null }, "c1"]],
        });
        let (status, body) = live.call(Method::POST, "/jmap", &token, Some(req)).await;
        assert_eq!(status, 200, "{body}");
        body["methodResponses"][0][1]["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|m| {
                m["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case("INBOX"))
            })
            .map(|m| m["unreadEmails"].as_u64().unwrap_or(0))
            .expect("an inbox")
    };
    assert_eq!(unread(alice.to_string()).await, 1, "alice sees her own");
    assert_eq!(unread(bob.to_string()).await, 0, "bob sees only his");
}

/// Webhook ids come from one counter for everybody; another account's id
/// is not found.
async fn webhooks_are_deleted_only_by_their_owner(live: &live::Live, alice: &str, bob: &str) {
    let (status, created) = live
        .call(
            Method::POST,
            "/api/agent/webhooks",
            alice,
            Some(serde_json::json!({ "url": "https://hooks.example/a", "event_type": "new_mail" })),
        )
        .await;
    assert!((200..300).contains(&status), "create: {status} {created}");
    let id = created["id"].as_i64().expect("id");
    let path = format!("/api/agent/webhooks/{id}");

    let (status, _) = live.call(Method::DELETE, &path, bob, None).await;
    assert_eq!(status, 404, "bob deleting alice's webhook");
    let (_, list) = live
        .call(Method::GET, "/api/agent/webhooks", alice, None)
        .await;
    assert!(
        list.to_string().contains("hooks.example/a"),
        "alice's webhook survived: {list}"
    );

    let (status, _) = live.call(Method::DELETE, &path, alice, None).await;
    assert_eq!(status, 204, "alice deleting her own");
}
