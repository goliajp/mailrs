//! Mail arriving in a conversation held for review rings no phone on the
//! SQL core either, and the badge does not move; its new-mail event still
//! reaches the clients, so Review refreshes, marked held, so they raise no
//! notice. Once released, the next message is news again.
//!
//! One test, because the push key and the shared store are read from the
//! environment once per process.

#![cfg(feature = "spg")]

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::notify;
use mailrs_mailbox::pg::{PgMailboxStore, ThreadQuery};
use mailrs_mailbox::scope::{Bucket, Scope};
use spg_sqlx::SpgPoolExt;

const USER: &str = "bob@x.com";
const TID: &str = "p1@outside.example";

fn mail(mid: &str, reply_to: Option<&str>) -> Vec<u8> {
    let thread = reply_to
        .map(|r| format!("In-Reply-To: <{r}>\r\nReferences: <{r}>\r\n"))
        .unwrap_or_default();
    format!(
        "From: Bank <alerts@outside.example>\r\nTo: {USER}\r\n\
Subject: Your account is locked\r\nMessage-ID: <{mid}>\r\n{thread}\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\r\nConfirm your password here.\r\n"
    )
    .into_bytes()
}

fn envelope(verdict: Option<String>) -> SpoolEnvelope {
    SpoolEnvelope {
        reverse_path: "alerts@outside.example".into(),
        forward_paths: vec![USER.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: verdict,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    }
}

fn held() -> Option<String> {
    Some(
        serde_json::json!({
            "rules_version": "2026-11-05.1",
            "score": 9.5,
            "quarantined": true,
            "layers": [],
        })
        .to_string(),
    )
}

/// Every push decision for `user` today, on the Web Push channel,
/// whatever its outcome. The subscription points at a loopback address
/// the sender refuses, so an attempt counts as failed.
fn pushes(kv: &mut kevy_client::Connection) -> i64 {
    let now = chrono::Utc::now().timestamp();
    notify::stats(kv, USER, 1, now).unwrap()[0]
        .counts
        .get("web")
        .map(|c| c.values().sum())
        .unwrap_or(0)
}

/// Wait for spawned pushes to be counted: `want` is reached, or the count
/// after five seconds is returned, and a little longer so one too many
/// would be counted too.
async fn pushes_settled(kv: &mut kevy_client::Connection, want: i64) -> i64 {
    for _ in 0..50 {
        if pushes(kv) >= want {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    pushes(kv)
}

/// The `held` flag of every new-mail event about [`TID`] so far, oldest
/// first; `None` for an event that does not say.
fn held_flags(frames: &mailrs_test_docker::kevy::Published) -> Vec<Option<bool>> {
    frames
        .all()
        .iter()
        .map(|v| serde_json::from_slice::<serde_json::Value>(v).unwrap())
        .filter(|e| e["type"] == "NewMessage" && e["thread_id"] == TID)
        .map(|e| e["held"].as_bool())
        .collect()
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

/// The Inbox part of the badge, as the core counts it.
async fn badge(mailbox: &PgMailboxStore) -> usize {
    let q = ThreadQuery {
        scope: Scope::Bucket(Bucket::Inbox),
        unread: true,
        ..ThreadQuery::all(USER)
    };
    mailbox.thread_rows(&q).await.unwrap().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_into_a_held_conversation_rings_no_phone_until_it_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let spool = tmp.path().join("spool");
    std::fs::create_dir_all(spool.join("incoming/new")).unwrap();
    let key = vapid_key(tmp.path());
    // a kevy-server: the new-mail event is written by a pipeline, which the
    // in-process store does not run
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let frames =
        mailrs_test_docker::kevy::listen(&url, mailrs_core_sidestate::keys::events::EVENTS_CHANNEL);
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &url);
        std::env::set_var("MAILRS_VAPID_PRIVATE_KEY_PATH", &key);
        std::env::set_var("MAILRS_VAPID_SUBJECT", "mailto:ops@x.com");
    }
    let mut kv = kevy_client::Connection::connect(&url).unwrap();
    notify::add_web_subscription(
        &mut kv,
        USER,
        &notify::WebSubscription {
            endpoint: format!("https://127.0.0.1/push/{USER}"),
            p256dh: "BAAA".into(),
            auth: "AAAA".into(),
            created_at: 0,
        },
    )
    .unwrap();

    let pool = spg_sqlx::SpgPool::connect_in_memory().await.unwrap();
    sqlx::raw_sql(include_str!("../../../scripts/init-schema.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO domains (name) VALUES ('x.com')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO accounts (address, domain) VALUES ($1, 'x.com')")
        .bind(USER)
        .execute(&pool)
        .await
        .unwrap();
    let mailbox = PgMailboxStore::new(pool.clone());
    let held_now = |pool: spg_sqlx::SpgPool| async move {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM quarantine_holds WHERE account_address = $1 AND thread_id = $2",
        )
        .bind(USER)
        .bind(TID)
        .fetch_one(&pool)
        .await
        .unwrap();
        n > 0
    };
    let drain = std::sync::Arc::new(mailrs_server::test_support::spool_drain(
        pool.clone(),
        maildir.to_string_lossy().into_owned(),
        Some(url.clone()),
        &spool,
    ));
    let arrive = |name: &str, env: SpoolEnvelope, body: Vec<u8>| {
        let at = spool.join("incoming/new").join(name);
        std::fs::write(at, encode_spool_blob(&env, &body)).unwrap();
        let drain = drain.clone();
        async move { tokio::task::spawn_blocking(move || drain()).await.unwrap() }
    };

    // held on arrival: nothing rings, as before
    assert_eq!(arrive("1.host", envelope(held()), mail(TID, None)).await, 1);
    assert!(held_now(pool.clone()).await, "the verdict did not hold it");
    assert_eq!(pushes_settled(&mut kv, 0).await, 0);
    assert_eq!(held_flags(&frames), [Some(true)], "the hold's event");
    let badge_held = badge(&mailbox).await;

    // a follow-up with no verdict of its own, into the held conversation
    let follow_up = mail("p2@outside.example", Some(TID));
    assert_eq!(arrive("2.host", envelope(None), follow_up).await, 1);
    assert!(held_now(pool.clone()).await, "the follow-up released it");
    assert_eq!(
        pushes_settled(&mut kv, 0).await,
        0,
        "held mail rang the phone"
    );
    assert_eq!(
        badge(&mailbox).await,
        badge_held,
        "held mail moved the badge"
    );
    // the clients still hear of it, to refresh Review, told it is held
    assert_eq!(
        held_flags(&frames),
        [Some(true), Some(true)],
        "the follow-up's event"
    );

    // released, the next message is news again
    assert_eq!(mailbox.release_thread(USER, TID).await.unwrap(), 1);
    let next = mail("p3@outside.example", Some(TID));
    assert_eq!(arrive("3.host", envelope(None), next).await, 1);
    assert_eq!(
        pushes_settled(&mut kv, 1).await,
        1,
        "a released conversation rang nothing"
    );
    assert_eq!(
        badge(&mailbox).await,
        badge_held + 1,
        "released, and not in the badge"
    );
    assert_eq!(
        held_flags(&frames),
        [Some(true), Some(true), Some(false)],
        "after the release, an ordinary event"
    );
}
