//! Deleting a conversation removes every file of the deleter's copies,
//! and leaves other owners able to open what they hold.
//!
//! One Message-ID can sit in a mailbox several times: the Sent copy and the
//! inbox copy of mail you sent yourself, or one inbox copy per alias a
//! message came through. The row names one file. A delete that unlinked
//! only that one left the rest on disk, and self-heal rebuilt the
//! conversation from them within minutes (prod, 2026-10-25). The same
//! delete also took the shared records with it, so the other owners of the
//! conversation lost it until self-heal rebuilt theirs.
//!
//! Driven the way production runs it: files on disk, the self-heal builds
//! the index, the backfill records the copies, the core route deletes, and
//! the self-heal runs again.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

const BOB: &str = "bob@x.com";
const ALICE: &str = "alice@x.com";
const PASSWORD: &str = "correct horse battery";

fn message(mid: &str, subject: &str) -> String {
    format!(
        "From: bob@x.com\r\nTo: bob@x.com, alice@x.com\r\nSubject: {subject}\r\n\
         Message-ID: <{mid}>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n"
    )
}

fn deliver(root: &std::path::Path, user: &str, folder: &str, raw: &str) {
    let (local, domain) = user.split_once('@').unwrap();
    let dir = root.join(domain).join(local).join(folder);
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(leaf)).unwrap();
    }
    mailrs_maildir::Maildir::open(&dir)
        .deliver(raw.as_bytes())
        .unwrap();
}

/// How many files in `user`'s mailbox, any folder, state `mid`.
fn files_with(root: &std::path::Path, user: &str, mid: &str) -> usize {
    let (local, domain) = user.split_once('@').unwrap();
    let needle = format!("Message-ID: <{mid}>");
    walkdir(&root.join(domain).join(local))
        .into_iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains(&needle))
                .unwrap_or(false)
        })
        .count()
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else if p
            .parent()
            .is_some_and(|d| d.ends_with("cur") || d.ends_with("new"))
        {
            out.push(p);
        }
    }
    out
}

async fn call(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    method: &str,
    uri: &str,
) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "{method} {uri}: {}",
        res.status()
    );
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn a_deleted_thread_takes_every_copy_and_spares_other_owners() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    // bob wrote to himself: a Sent copy and an inbox copy
    deliver(&root, BOB, ".Sent", &message("self@x.com", "note to self"));
    deliver(&root, BOB, "", &message("self@x.com", "note to self"));
    // a message that reached bob three times, and alice once
    for _ in 0..3 {
        deliver(&root, BOB, "", &message("shared@x.com", "team news"));
    }
    deliver(&root, ALICE, "", &message("shared@x.com", "team news"));
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/alice/.Trash").join(leaf)).unwrap();
    }

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    let hash = mailrs_core::users::UserStore::hash_password(PASSWORD).unwrap();
    let blob = serde_json::json!({ "password_hash": hash, "active": true }).to_string();
    mailbox.upsert_account(BOB, &blob).unwrap();
    mailbox.upsert_account(ALICE, &blob).unwrap();
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    mailrs_fastcore::self_heal_once(&state, BOB).await;
    // alice's copy arrives the way delivery files it: the self-heal only
    // builds a conversation nobody has yet, so it cannot give a second
    // owner a thread bob's pass already created
    let alice_file = walkdir(&root.join("x.com").join("alice"))
        .pop()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let shared_row = state
        .mailbox
        .user_message_view(BOB, "shared@x.com")
        .unwrap()
        .expect("bob's row");
    let shared_tid_for_alice =
        serde_json::from_slice::<serde_json::Value>(&shared_row).unwrap()["thread_id"]
            .as_str()
            .unwrap()
            .to_string();
    state
        .mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: &shared_tid_for_alice,
                user: ALICE,
                subject: "team news",
                senders_csv: "bob@x.com",
                latest_date: 1_786_669_200,
                latest_preview: "body",
                category: "inbox",
                unread: true,
                is_own: false,
            },
            "shared@x.com",
            &shared_row,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &alice_file,
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
    state
        .mailbox
        .register_uid(ALICE, 1, "shared@x.com")
        .unwrap();
    state
        .mailbox
        .set_thread_for_message_id(ALICE, "shared@x.com", &shared_tid_for_alice)
        .unwrap();

    let self_tid = state
        .mailbox
        .thread_for_message_id(BOB, "self@x.com")
        .unwrap()
        .expect("self thread");
    let shared_tid = state
        .mailbox
        .thread_for_message_id(BOB, "shared@x.com")
        .unwrap()
        .expect("shared thread");
    assert_eq!(
        state
            .mailbox
            .thread_for_message_id(ALICE, "shared@x.com")
            .unwrap()
            .as_deref(),
        Some(shared_tid.as_str()),
        "the fixture needs one conversation two people own"
    );

    // the backfill records every copy; a second run has nothing to add
    let first = call(
        &state,
        "POST",
        "/v1/admin/maintenance:backfill-message-files",
    )
    .await;
    assert_eq!(
        first["total"]["walked"], 6,
        "bob's five files and alice's one: {first}"
    );
    assert_eq!(first["total"]["no_row"], 0, "{first}");
    assert_eq!(
        first["total"]["added"].as_u64().unwrap() + first["total"]["already"].as_u64().unwrap(),
        6,
        "{first}"
    );
    let second = call(
        &state,
        "POST",
        "/v1/admin/maintenance:backfill-message-files",
    )
    .await;
    assert_eq!(second["total"]["added"], 0, "{second}");

    // bob deletes both conversations through the core route
    for tid in [&self_tid, &shared_tid] {
        let uri = format!("/v1/users/{BOB}/threads/{}", urlencode(tid));
        call(&state, "DELETE", &uri).await;
    }
    assert_eq!(
        files_with(&root, BOB, "self@x.com"),
        0,
        "a copy of the self-sent mail survived"
    );
    assert_eq!(
        files_with(&root, BOB, "shared@x.com"),
        0,
        "an alias copy survived"
    );

    // and they stay gone after the sweep that used to bring them back
    mailrs_fastcore::self_heal_once(&state, BOB).await;
    assert!(
        state
            .mailbox
            .get_thread_for_user(BOB, &self_tid)
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .mailbox
            .get_thread_for_user(BOB, &shared_tid)
            .unwrap()
            .is_none()
    );

    // alice still has hers: the file, the row, and IMAP serves it
    assert_eq!(files_with(&root, ALICE, "shared@x.com"), 1);
    assert!(
        state
            .mailbox
            .get_thread_for_user(ALICE, &shared_tid)
            .unwrap()
            .is_some()
    );
    assert!(
        state
            .mailbox
            .user_message_view(ALICE, "shared@x.com")
            .unwrap()
            .is_some()
    );

    let dir = Arc::new(mailrs_fastcore::FastcoreDirectory::new(
        state.clone(),
        root.clone(),
    ));
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir, server));
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    let mut until = async |tag: &str, cmd: &str| {
        w.write_all(format!("{tag} {cmd}\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        loop {
            let mut l = String::new();
            assert!(r.read_line(&mut l).await.unwrap() > 0, "closed: {out}");
            out.push_str(&l);
            if l.starts_with(&format!("{tag} ")) {
                assert!(l.contains(" OK "), "{cmd}: {out}");
                return out;
            }
        }
    };
    until("a1", &format!("LOGIN {ALICE} \"{PASSWORD}\"")).await;
    until("a2", "SELECT INBOX").await;
    let fetched = until("a3", "FETCH 1 BODY[]").await;
    assert!(fetched.contains("Message-ID: <shared@x.com>"), "{fetched}");

    // a copy IMAP makes is a copy the next delete has to find
    until("a4", "COPY 1 Trash").await;
    let set = mailrs_mailbox_kevy::keys::user_message_files(ALICE, "shared@x.com");
    let names = state.mailbox.store_ref().smembers(set.as_bytes()).unwrap();
    assert_eq!(names.len(), 2, "the Trash copy was not recorded: {names:?}");
    assert!(names.iter().any(|n| n.starts_with(b".Trash/")), "{names:?}");
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
