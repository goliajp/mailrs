//! Mail arriving in a conversation held for review rings no phone, and the
//! badge does not move; its new-mail event still reaches the clients, so
//! Review refreshes, marked held, so they raise no notice. Once the
//! conversation is released, the next message is news again.
//!
//! The follow-up carries no verdict of its own, so "this message was held"
//! alone let it ring the phone with the scam's sender and subject.
//!
//! One test, because the push key and the shared store are read from the
//! environment once per process.

use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::notify;

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
fn held_flags(kv: &mut kevy_client::Connection) -> Vec<Option<bool>> {
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
    let nanos = |k: &[u8]| -> u128 { String::from_utf8_lossy(&k[prefix.len()..]).parse().unwrap() };
    keys.sort_by_key(|k| nanos(k));
    keys.iter()
        .filter_map(|k| kv.get(k).unwrap())
        .map(|v| serde_json::from_slice::<serde_json::Value>(&v).unwrap())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mail_into_a_held_conversation_rings_no_phone_until_it_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let incoming = tmp.path().join("spool/incoming/new");
    std::fs::create_dir_all(&incoming).unwrap();
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(maildir.join("x.com/bob").join(leaf)).unwrap();
    }
    let key = vapid_key(tmp.path());
    // a kevy-server: the new-mail event is written by a pipeline, which the
    // in-process store does not run
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

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
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
    let badge = |state: &mailrs_fastcore::FastcoreState| {
        state
            .mailbox
            .count_flag_in_buckets(USER, "unread", &["inbox"])
            .unwrap()
    };

    // held on arrival: nothing rings, as before
    assert_eq!(arrive("1.host", envelope(held()), mail(TID, None)).await, 1);
    let thread = state.mailbox.get_thread_for_user(USER, TID).unwrap();
    assert!(
        thread.is_some_and(|t| t.quarantined),
        "the verdict did not hold it"
    );
    assert_eq!(pushes_settled(&mut kv, 0).await, 0);
    assert_eq!(held_flags(&mut kv), [Some(true)], "the hold's event");
    let badge_held = badge(&state);

    // a follow-up with no verdict of its own, into the held conversation
    let follow_up = mail("p2@outside.example", Some(TID));
    assert_eq!(arrive("2.host", envelope(None), follow_up).await, 1);
    let thread = state.mailbox.get_thread_for_user(USER, TID).unwrap();
    assert!(
        thread.is_some_and(|t| t.quarantined),
        "the follow-up released it"
    );
    assert_eq!(
        pushes_settled(&mut kv, 0).await,
        0,
        "held mail rang the phone"
    );
    assert_eq!(badge(&state), badge_held, "held mail moved the badge");
    // the clients still hear of it, to refresh Review, told it is held
    assert_eq!(
        held_flags(&mut kv),
        [Some(true), Some(true)],
        "the follow-up's event"
    );

    // released, the next message is news again
    assert!(state.mailbox.set_quarantined(USER, TID, false).unwrap());
    let next = mail("p3@outside.example", Some(TID));
    assert_eq!(arrive("3.host", envelope(None), next).await, 1);
    assert_eq!(
        pushes_settled(&mut kv, 1).await,
        1,
        "a released conversation rang nothing"
    );
    assert_eq!(
        badge(&state),
        badge_held + 1,
        "released, and not in the badge"
    );
    assert_eq!(
        held_flags(&mut kv),
        [Some(true), Some(true), Some(false)],
        "after the release, an ordinary event"
    );
}
