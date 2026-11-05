//! A new message in a conversation two accounts share notifies only the
//! account it was delivered to: its Web Push decisions and its new-mail
//! event name that account, and the other member, who did not receive it,
//! is told nothing about it.
//!
//! One test, because the push key and the shared store are read from the
//! environment once per process.

use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::notify;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";
const TID: &str = "r1@outside.example";

fn mail(mid: &str, to: &str, subject: &str, reply_to: Option<&str>) -> Vec<u8> {
    let thread = reply_to
        .map(|r| format!("In-Reply-To: <{r}>\r\nReferences: <{r}>\r\n"))
        .unwrap_or_default();
    format!(
        "From: Carol <carol@outside.example>\r\nTo: {to}\r\n\
Subject: {subject}\r\nMessage-ID: <{mid}>\r\n{thread}\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\r\nSee you on Friday.\r\n"
    )
    .into_bytes()
}

fn envelope(to: &[&str]) -> SpoolEnvelope {
    SpoolEnvelope {
        reverse_path: "carol@outside.example".into(),
        forward_paths: to.iter().map(|t| t.to_string()).collect(),
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    }
}

/// Every Web Push decision for `user` today, whatever its outcome. The
/// subscription points at a loopback address the sender refuses.
fn pushes(kv: &mut kevy_client::Connection, user: &str) -> i64 {
    let now = chrono::Utc::now().timestamp();
    notify::stats(kv, user, 1, now).unwrap()[0]
        .counts
        .get("web")
        .map(|c| c.values().sum())
        .unwrap_or(0)
}

/// Wait for spawned pushes to be counted: `want` is reached for both, or
/// the counts after five seconds, and a little longer so one too many
/// would be counted too.
async fn pushes_settled(kv: &mut kevy_client::Connection, want: (i64, i64)) -> (i64, i64) {
    for _ in 0..50 {
        if pushes(kv, ALICE) >= want.0 && pushes(kv, BOB) >= want.1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    (pushes(kv, ALICE), pushes(kv, BOB))
}

/// `(user, subject)` of every new-mail event so far.
fn events(kv: &mut kevy_client::Connection) -> Vec<(String, String)> {
    let prefix = mailrs_core_sidestate::keys::events::NOTIFY_PREFIX;
    let mut pattern = prefix.to_vec();
    pattern.push(b'*');
    let mut keys = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, batch) = kv.scan(cursor, Some(pattern.as_slice()), None).unwrap();
        keys.extend(batch);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    let mut out: Vec<(String, String)> = keys
        .iter()
        .filter_map(|k| kv.get(k).unwrap())
        .map(|v| serde_json::from_slice::<serde_json::Value>(&v).unwrap())
        .filter(|e| e["type"] == "NewMessage")
        .map(|e| {
            (
                e["user"].as_str().unwrap_or("").to_string(),
                e["subject"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

fn vapid_key(dir: &std::path::Path) -> std::path::PathBuf {
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
    use base64::Engine as _;
    let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(doc.as_ref());
    let path = dir.join("vapid.pem");
    std::fs::write(
        &path,
        format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n"),
    )
    .unwrap();
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_for_one_member_notifies_only_them() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let incoming = tmp.path().join("spool/incoming/new");
    std::fs::create_dir_all(&incoming).unwrap();
    for user in ["alice", "bob"] {
        for leaf in ["new", "cur", "tmp"] {
            std::fs::create_dir_all(maildir.join("x.com").join(user).join(leaf)).unwrap();
        }
    }
    let key = vapid_key(tmp.path());
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &url);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "x.com");
        std::env::set_var("MAILRS_VAPID_PRIVATE_KEY_PATH", &key);
        std::env::set_var("MAILRS_VAPID_SUBJECT", "mailto:ops@x.com");
    }
    let mut kv = kevy_client::Connection::connect(&url).unwrap();
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    for user in [ALICE, BOB] {
        notify::add_web_subscription(
            &mut kv,
            user,
            &notify::WebSubscription {
                endpoint: format!("https://127.0.0.1/push/{user}"),
                p256dh: "BAAA".into(),
                auth: "AAAA".into(),
                created_at: 0,
            },
        )
        .unwrap();
        mailbox
            .upsert_account(user, &format!(r#"{{"address":"{user}","active":true}}"#))
            .unwrap();
    }
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(url.clone());
    let state = Arc::new(state);
    let arrive = |name: &str, env: SpoolEnvelope, body: Vec<u8>| {
        std::fs::write(incoming.join(name), encode_spool_blob(&env, &body)).unwrap();
        let (state, incoming) = (state.clone(), incoming.clone());
        let root = maildir.to_string_lossy().into_owned();
        async move {
            tokio::task::spawn_blocking(move || {
                mailrs_fastcore::spool_drain::drain_once(&incoming, &root, &state).0
            })
            .await
            .unwrap()
        }
    };

    let to_both = mail(TID, "alice@x.com, bob@x.com", "Friday", None);
    assert_eq!(arrive("1.host", envelope(&[ALICE, BOB]), to_both).await, 1);
    assert_eq!(pushes_settled(&mut kv, (1, 1)).await, (1, 1), "premise");

    // a reply into the same conversation, for Alice only
    let to_alice = mail(
        "r2@outside.example",
        ALICE,
        "Re: Friday (private)",
        Some(TID),
    );
    assert_eq!(arrive("2.host", envelope(&[ALICE]), to_alice).await, 1);
    for user in [ALICE, BOB] {
        assert!(
            state
                .mailbox
                .get_thread_for_user(user, TID)
                .unwrap()
                .is_some(),
            "premise: {user} holds the conversation"
        );
    }
    assert_eq!(
        pushes_settled(&mut kv, (2, 1)).await,
        (2, 1),
        "(Alice, Bob) pushes: Bob was notified of mail he never received"
    );
    assert_eq!(
        events(&mut kv),
        [
            (ALICE.to_string(), "Friday".to_string()),
            (ALICE.to_string(), "Re: Friday (private)".to_string()),
            (BOB.to_string(), "Friday".to_string()),
        ],
        "new-mail events"
    );
}
