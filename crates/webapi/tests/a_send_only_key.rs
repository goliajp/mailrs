//! A `mail:send` key, made by the host command, sends as its owner and
//! does nothing else.
//!
//! It is what a service gets to send in a person's name: the message goes
//! out with the owner as the envelope sender, the copy lands in the
//! owner's Sent, and every send leaves an audit line. The key cannot read
//! the owner's mail, cannot reach MCP, cannot list the owner's other
//! sends, and cannot be turned into a sign-in.
//!
//! Against a real kevy-server: the queue write is pipelined, which the
//! in-process kevy does not support.

mod live;

use mailrs_core_sidestate::families::outbound as ob;
use reqwest::Method;

const OWNER: &str = "owner@example.test";

fn file_count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_only_key_sends_as_its_owner_and_reaches_nothing_else() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    let live_kevy = kevy.clone();
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&live_kevy).await;
    // SAFETY: as above; the host command reads these to find the core
    unsafe {
        std::env::set_var("MAILRS_CORE_RPC_BASE", &live.core_url);
        std::env::set_var(mailrs_core_api::AUTH_SECRET_ENV, "secret");
    }
    live.add_account(OWNER, "pw").await;
    let session = live.login(OWNER, "pw").await;

    assert!(
        mailrs_webapi::cli::create_send_key("nobody@example.test", "x")
            .await
            .is_err(),
        "a key for an address that is not an account"
    );
    let key = mailrs_webapi::cli::create_send_key(OWNER, "weekly report")
        .await
        .expect("create key");
    assert!(key.starts_with("mk_"));

    // it shows on the owner's key list, where the owner can revoke it
    let (status, keys) = live
        .call(Method::GET, "/api/agent/keys", &session, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(keys["items"][0]["scopes"], serde_json::json!(["mail:send"]));
    assert_eq!(keys["items"][0]["prefix"], key[..8]);

    // the owner sends one by hand, the key sends one
    let mail = |subject: &str| {
        serde_json::json!({
            "from": OWNER,
            "to": ["bob@example.test"],
            "subject": subject,
            "body": "hello",
        })
    };
    let (status, mine) = live
        .call(
            Method::POST,
            "/api/mail/send",
            &session,
            Some(mail("by hand")),
        )
        .await;
    assert_eq!(status, 200, "{mine}");
    let res = live
        .http
        .post(format!("{}/api/mail/send", live.base))
        .bearer_auth(&key)
        .header("x-caller", "insight-weekly")
        .json(&mail("weekly report"))
        .send()
        .await
        .expect("send");
    assert_eq!(res.status(), 200);
    let sent: serde_json::Value = res.json().await.expect("body");
    let message_id = sent["message_id"].as_str().expect("message_id").to_string();

    // both copies are in the owner's Sent
    let sent_dir = tmp.path().join("example.test/owner/.Sent/cur");
    assert_eq!(file_count(&sent_dir), 2);

    // the envelope sender of the key's send is the owner
    let mut c = kevy_client::Connection::connect(&kevy).expect("kevy");
    let mut senders = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, keys) = c
            .scan(cursor, Some(ob::JOB_PATTERN), Some(100))
            .expect("scan");
        for k in keys {
            let blob = c.hget(&k, b"blob").expect("hget").unwrap_or_default();
            let job: serde_json::Value = serde_json::from_slice(&blob).expect("job blob");
            if job["send_id"] == message_id.as_str() {
                senders.push(job["sender"].as_str().expect("sender").to_string());
            }
        }
        if next == 0 {
            break;
        }
        cursor = next;
    }
    assert_eq!(senders, vec![OWNER.to_string()]);

    // the audit line names the key, the caller and the message
    let audit = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit");
    let lines: Vec<serde_json::Value> = audit
        .iter()
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect();
    let line = lines
        .iter()
        .find(|l| l["action"] == "mail.send.agent_key")
        .expect("send audit line");
    assert_eq!(line["actor"], OWNER);
    assert_eq!(line["target"], message_id.as_str());
    let detail = line["detail"].as_str().expect("detail");
    assert!(
        detail.starts_with(&format!("key={} caller=insight-weekly ip=", &key[..8])),
        "{detail}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l["action"] == "agent_key.create" && l["target"] == OWNER),
        "{lines:?}"
    );

    // the key lists only the send it made; the owner sees both
    let (status, by_key) = live.call(Method::GET, "/api/mail/sends", &key, None).await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = by_key
        .as_array()
        .expect("list")
        .iter()
        .map(|s| s["send_id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, vec![message_id.as_str()]);
    let (_, by_owner) = live
        .call(Method::GET, "/api/mail/sends", &session, None)
        .await;
    assert_eq!(by_owner.as_array().expect("list").len(), 2);

    // nothing else answers it
    for (method, path) in [
        (Method::GET, "/api/conversations"),
        (Method::GET, "/api/conversations/search?q=hello"),
        (
            Method::GET,
            format!("/api/mail/sends/{message_id}/source").as_str(),
        ),
        (
            Method::DELETE,
            format!("/api/mail/sends/{message_id}").as_str(),
        ),
        (Method::GET, "/api/contacts"),
        (Method::GET, "/api/mail/signatures"),
        (Method::GET, "/api/mail/sent"),
        (Method::GET, "/api/mail/folders"),
        (Method::GET, "/api/mail/stats"),
        (Method::GET, "/api/mail/export"),
        (Method::GET, "/api/auth/me"),
        (Method::GET, "/api/admin/audit-log"),
        (Method::GET, "/api/admin/audit/accounts"),
        (Method::GET, "/api/agent/keys"),
        (Method::POST, "/api/agent/keys"),
    ] {
        let (status, _) = live.call(method.clone(), path, &key, None).await;
        assert_eq!(status, 403, "{method} {path}");
    }
    let mcp = live
        .http
        .post(format!("{}/mcp", live.base))
        .bearer_auth(&key)
        .header("accept", "application/json, text/event-stream")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "t", "version": "0" },
            },
        }))
        .send()
        .await
        .expect("mcp");
    assert_eq!(mcp.status(), 403);

    // a key is not a session: linking a sign-in method asks for one
    let header = |token: &str| {
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            "authorization",
            format!("Bearer {token}").parse().expect("header"),
        );
        h
    };
    assert!(
        mailrs_webapi::session::resolve_session_user_from_headers(&header(&key))
            .await
            .is_none()
    );
    assert_eq!(
        mailrs_webapi::session::resolve_session_user_from_headers(&header(&session))
            .await
            .map(|(address, _)| address)
            .as_deref(),
        Some(OWNER)
    );

    // the host command revokes it, and it stops working at once
    let id = keys["items"][0]["id"].as_i64().expect("id");
    assert!(
        mailrs_webapi::cli::revoke_key(OWNER, id + 100)
            .await
            .is_err(),
        "an unknown key"
    );
    mailrs_webapi::cli::revoke_key(OWNER, id)
        .await
        .expect("revoke");
    let (status, _) = live.call(Method::GET, "/api/mail/sends", &key, None).await;
    assert_eq!(status, 401);
    let (_, keys) = live
        .call(Method::GET, "/api/agent/keys", &session, None)
        .await;
    assert_eq!(keys["items"], serde_json::json!([]));
    let audit = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, 0)
        .expect("audit");
    let line: serde_json::Value = serde_json::from_slice(&audit[0]).expect("line");
    assert_eq!(line["action"], "agent_key.revoke");
    assert_eq!(line["actor"], "host");
    assert_eq!(line["target"], OWNER);
    assert_eq!(
        line["detail"],
        format!("id={id} prefix={} name=weekly report", &key[..8])
    );
}
