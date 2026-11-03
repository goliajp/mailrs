//! A team inbox through the front door, with an internal note whose text is
//! a canary. The note is for the group's current members only, and its text
//! is stored in exactly one place: not in the outbound queue a reply goes
//! through, not in a webhook payload, not in the audit log, not in the
//! maildir files IMAP, POP3 and JMAP serve, and not in an mbox export. The
//! one way back to a deleted note's text is the super-administrator view,
//! and reading it is audited. Of two members replying on the same count,
//! only one goes out unless the second insists.

mod live;

use mailrs_core_sidestate::families::{email_groups, shared_inbox, webhooks};
use reqwest::Method;
use serde_json::json;

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const CAROL: &str = "carol@example.test";
const ADMIN: &str = "admin@example.test";
const CANARY: &str = "canary-7f3e1b-internal-only";
const GID: i64 = 1;

const ORDER: &str = "From 1@xxx Mon Jan 02 09:00:00 +0000 2023\n\
X-Gmail-Labels: Inbox,Unread\n\
Message-ID: <order1@oldmail.net>\n\
From: Erin <erin@oldmail.net>\n\
To: support@example.test\n\
Subject: Lunch order for Friday\n\
Date: Mon, 02 Jan 2023 09:00:00 +0000\n\
\n\
Two sandwiches, please.\n";

/// Every value in the network kevy, as `(key, bytes)`.
fn every_value(c: &mut kevy_client::Connection) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for key in c.keys(b"*").expect("keys") {
        let name = String::from_utf8_lossy(&key).to_string();
        let kind = c.type_of(&key).expect("type");
        let parts: Vec<Vec<u8>> = match kind.as_str() {
            "string" => c.get(&key).expect("get").into_iter().collect(),
            "hash" => c.hgetall(&key).expect("hgetall"),
            "list" => c.lrange(&key, 0, -1).expect("lrange"),
            "set" => c.smembers(&key).expect("smembers"),
            "zset" => c.zrange(&key, 0, -1).expect("zrange"),
            _ => Vec::new(),
        };
        for p in parts {
            out.push((name.clone(), p));
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn files_with(dir: &std::path::Path, needle: &str, found: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).expect("dir").flatten() {
        let p = e.path();
        if p.is_dir() {
            files_with(&p, needle, found);
        } else if contains(&std::fs::read(&p).unwrap_or_default(), needle) {
            found.push(p.display().to_string());
        }
    }
}

async fn import(live: &live::Live, bearer: &str) {
    let res = live
        .http
        .post(format!("{}/api/mail/import", live.base))
        .bearer_auth(bearer)
        .body(ORDER.as_bytes().to_vec())
        .send()
        .await
        .expect("import");
    let status = res.status();
    let report: serde_json::Value = res.json().await.expect("report");
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["imported"], 1, "{report}");
    assert_eq!(report["held"], 0, "{report}");
}

async fn export(live: &live::Live, bearer: &str) -> Vec<u8> {
    live.confirm(bearer, "pw").await;
    let res = live
        .http
        .get(format!("{}/api/mail/export", live.base))
        .bearer_auth(bearer)
        .send()
        .await
        .expect("export");
    assert_eq!(res.status(), 200);
    res.bytes().await.expect("mbox").to_vec()
}

async fn thread_of(live: &live::Live, user: &str) -> String {
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: 10,
            folder: Some("all".into()),
            ..Default::default()
        },
    };
    let rows = live
        .core
        .list_conversations(user, &req)
        .await
        .expect("list");
    let subjects: Vec<String> = rows.items.iter().map(|r| r.subject.clone()).collect();
    rows.items
        .into_iter()
        .find(|r| r.subject.contains("order"))
        .unwrap_or_else(|| panic!("the order is not listed for {user}: {subjects:?}"))
        .thread_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_stays_with_the_team_and_two_replies_need_a_second_thought() {
    // a real kevy-server: the outbound queue writes in transactions
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.mailbox.ensure_thread_table();
    for who in [ALICE, BOB, CAROL, ADMIN] {
        live.add_account(who, "pw").await;
    }
    live.make_super(ADMIN);
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;
    let carol = live.login(CAROL, "pw").await;
    let admin = live.login(ADMIN, "pw").await;

    // the group, and both members' copies tied to one team conversation
    let mut net = kevy_client::Connection::connect(&kevy).expect("kevy");
    let row = json!({ "id": GID, "address": "support@example.test", "name": "support" });
    net.hset(
        email_groups::GROUPS_KEY.as_bytes(),
        &[(b"1".as_slice(), row.to_string().as_bytes())],
    )
    .expect("group");
    net.sadd(
        email_groups::members_key(GID).as_bytes(),
        &[ALICE.as_bytes(), BOB.as_bytes()],
    )
    .expect("members");
    email_groups::set_shared(&mut net, GID, true).expect("shared");
    import(&live, &alice).await;
    import(&live, &bob).await;
    let conv = shared_inbox::conv_for_arrival(&mut net, GID, "<order1@oldmail.net>", "", &[])
        .expect("conv");
    for (who, token) in [(ALICE, &alice), (BOB, &bob)] {
        let tid = thread_of(&live, who).await;
        live.mailbox
            .set_shared_copy(who, &tid, &conv.tag(), "")
            .expect("tag");
        shared_inbox::record_copy(&mut net, conv, who, &tid).expect("copy");
        let (status, rows) = live
            .call(
                Method::GET,
                "/api/conversations?limit=10&folder=all",
                token,
                None,
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(rows[0]["shared_conv"], conv.tag(), "{rows}");
    }
    webhooks::create(
        &mut net,
        webhooks::NewWebhook {
            account_address: BOB.into(),
            url: "https://hooks.example/in".into(),
            event_type: "thread.assigned".into(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )
    .expect("webhook");
    let base = format!("/api/shared/{GID}/{}", conv.conv);

    // a note, seen by members only
    let (status, added) = live
        .call(
            Method::POST,
            &format!("{base}/notes"),
            &alice,
            Some(json!({ "text": CANARY })),
        )
        .await;
    assert_eq!(status, 200, "{added}");
    let (status, notes) = live
        .call(Method::GET, &format!("{base}/notes"), &bob, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(notes[0]["text"], CANARY);
    for outsider in [&carol, &admin] {
        let (status, _) = live
            .call(Method::GET, &format!("{base}/notes"), outsider, None)
            .await;
        assert_eq!(status, 403, "a non-member read the notes");
    }

    // assignment fires the webhook, without the note
    let (status, body) = live
        .call(
            Method::POST,
            &format!("{base}/assign"),
            &alice,
            Some(json!({ "to": BOB })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["rows"], 2);

    // two replies on the same count: one goes, the other must insist
    let tid = thread_of(&live, ALICE).await;
    let reply = |seen: u64, force: bool| {
        json!({
            "from": ALICE, "to": ["erin@oldmail.net"], "subject": "Re: Lunch order for Friday",
            "body": "It ships today.", "reply_to_thread_id": tid,
            "shared": { "gid": GID, "conv": conv.conv, "seen_seq": seen, "force": force },
        })
    };
    let (status, sent) = live
        .call(
            Method::POST,
            "/api/mail/send",
            &alice,
            Some(reply(0, false)),
        )
        .await;
    assert_eq!(status, 200, "{sent}");
    let mut second = reply(0, false);
    second["from"] = json!(BOB);
    let (status, refused) = live
        .call(Method::POST, "/api/mail/send", &bob, Some(second.clone()))
        .await;
    assert_eq!(status, 409, "{refused}");
    assert_eq!(refused["error"], "already_replied");
    assert_eq!(refused["by"], ALICE);
    second["shared"]["force"] = json!(true);
    let (status, forced) = live
        .call(Method::POST, "/api/mail/send", &bob, Some(second))
        .await;
    assert_eq!(status, 200, "{forced}");

    // the canary is in the notes and nowhere else
    let values = every_value(&mut net);
    assert!(
        values.iter().any(|(_, v)| contains(v, "thread.assigned")),
        "no webhook was queued for the assignment"
    );
    let mut replies = Vec::new();
    files_with(tmp.path(), "Re: Lunch order for Friday", &mut replies);
    assert!(
        !replies.is_empty()
            || values
                .iter()
                .any(|(_, v)| contains(v, "Re: Lunch order for Friday")),
        "the reply was neither filed nor queued"
    );
    let holders: Vec<String> = values
        .into_iter()
        .filter(|(_, v)| contains(v, CANARY))
        .map(|(k, _)| k)
        .collect();
    let notes_key = conv.key("notes");
    assert_eq!(
        holders,
        vec![notes_key],
        "the note's text is stored elsewhere"
    );
    let mut on_disk = Vec::new();
    files_with(tmp.path(), CANARY, &mut on_disk);
    assert!(
        on_disk.is_empty(),
        "a maildir file carries the note: {on_disk:?}"
    );
    for token in [&alice, &bob] {
        assert!(
            !contains(&export(&live, token).await, CANARY),
            "an export carries the note"
        );
    }

    // deleted: gone from the team, readable once by a super administrator
    let id = added["id"].as_i64().expect("id");
    let (status, _) = live
        .call(Method::DELETE, &format!("{base}/notes/{id}"), &bob, None)
        .await;
    assert_eq!(status, 403, "a note is deleted by its author only");
    let (status, _) = live
        .call(Method::DELETE, &format!("{base}/notes/{id}"), &alice, None)
        .await;
    assert_eq!(status, 204);
    let (_, notes) = live
        .call(Method::GET, &format!("{base}/notes"), &bob, None)
        .await;
    assert_eq!(notes, json!([]));
    let deleted = format!("/api/admin/shared/{GID}/{}/deleted-notes/{id}", conv.conv);
    let (status, _) = live.call(Method::GET, &deleted, &alice, None).await;
    assert_eq!(status, 403);
    let (status, original) = live.call(Method::GET, &deleted, &admin, None).await;
    assert_eq!(status, 200, "{original}");
    assert_eq!(original["versions"][0], CANARY);
    let audit: Vec<mailrs_core_sidestate::families::audit::AuditFact> = net
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect();
    assert!(
        audit
            .iter()
            .any(|f| f.action == "shared_note.read_deleted" && f.actor == ADMIN)
    );
    assert!(audit.iter().all(|f| !f.detail.contains(CANARY)));

    // taken out of the group: the team is gone, the mail is not
    net.srem(email_groups::members_key(GID).as_bytes(), &[BOB.as_bytes()])
        .expect("remove");
    let (status, _) = live
        .call(Method::GET, &format!("{base}/notes"), &bob, None)
        .await;
    assert_eq!(status, 403);
    let (status, _) = live.call(Method::GET, &base, &bob, None).await;
    assert_eq!(status, 403);
    assert!(thread_of(&live, BOB).await.contains("order1"));
}
