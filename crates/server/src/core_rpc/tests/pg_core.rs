//! In-process validation that the PG core router mounts + serves the
//! v2 dual-mode routes (deliver_message ingest + read-back +
//! Message-ID idempotency) against a real in-memory spg store.
//! Spawns the router on an ephemeral port and drives it with the
//! core-api Client — the same surface `mailrs-core-sync` uses.

#![cfg(feature = "spg")]

// Named, and addressed from the crate root rather than through `super`.
// These tests were inline in `core_rpc/mod.rs` until 2026-08-02, where
// `super::*` reached `core_rpc`; moving them into `tests/` shifted `super`
// one level down onto `core_rpc::tests`, which holds nothing but the module
// declarations — so the glob resolved to nothing and every name here went
// missing. A crate-rooted path cannot be broken by moving this file again.
use std::sync::Arc;

use crate::core_rpc::{CoreRpcState, build_full_router};

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use spg_sqlx::SpgPoolExt;

// Anchored to the crate directory, not to this file's own depth. The same
// 2026-08-02 move left this path one `..` short and it read as
// `crates/scripts/init-schema.sql`. `request_contract.rs` in webapi reaches
// the same repo-level fixtures this way for the same reason.
const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

pub(super) async fn spawn_pg_core() -> String {
    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply init-schema.sql");
    // domain FK for account inserts
    sqlx::query("INSERT INTO domains (name) VALUES ('test') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();

    let mailbox = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone()));
    let domain = Arc::new(crate::domain_store::DomainStore::new(
        Some(pool.clone()),
        None,
        crate::health::HealthState::new(),
    ));
    let state = Arc::new(CoreRpcState {
        mailbox,
        domain,
        pool,
        maildir_root: "/tmp/pg-core-test".into(),
        net_url: None,
    });
    let router = build_full_router(state, String::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

pub(super) fn deliver_req(
    message_id: &str,
    uid: u32,
    thread_id: &str,
    user: &str,
) -> DeliverMessageRequest {
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid,
        "blob_ref": format!("{message_id}.host"),
        "sender": "remote@x.y", "recipients": user, "subject": "Hi",
        "date": 1_700_000_000i64, "internal_date": 1_700_000_000i64,
        "size": 42, "flags": 1, "message_id": message_id,
        "in_reply_to": "", "thread_id": thread_id, "modseq": 0,
        "user_address": user,
    });
    DeliverMessageRequest {
        message_id: message_id.into(),
        subject: "Hi".into(),
        senders_csv: "remote@x.y".into(),
        latest_date: 1_700_000_000,
        latest_preview: String::new(),
        category: "inbox".into(),
        unread: true,
        uid,
        payload_wire_json: wire.to_string(),
    }
}

#[tokio::test]
async fn pg_core_ingest_reads_back_and_is_idempotent() {
    let base = spawn_pg_core().await;
    let c = Client::new(base, String::new());
    let user = "u@test";
    let thread = "t1@test";

    c.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "U".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");

    // ingest a message via the P0 route
    c.deliver_message(user, thread, &deliver_req("m1@test", 1, thread, user))
        .await
        .expect("deliver_message");

    // read it back over the contract
    let msgs = c.list_thread_messages(user, thread).await.expect("list");
    assert_eq!(msgs.items.len(), 1, "ingested message must read back");
    assert_eq!(msgs.items[0].message_id, "m1@test");

    // Message-ID idempotent: re-deliver is a no-op, still one message
    c.deliver_message(user, thread, &deliver_req("m1@test", 1, thread, user))
        .await
        .expect("re-deliver");
    let msgs2 = c.list_thread_messages(user, thread).await.expect("list2");
    assert_eq!(msgs2.items.len(), 1, "re-delivery must not duplicate");
}

// ── real cross-backend sync (v2 audit point 5) ──────────────────
// Spawn a live kevy fastcore AND the spg pg-core, then run the real
// mailrs-core-sync between them over HTTP. This is the actual
// production switch axis (kevy↔pg), which the kevy↔kevy roundtrip
// test could not cover.

pub(super) fn spawn_fastcore() -> String {
    use std::sync::Arc as StdArc;
    let store = StdArc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let state = StdArc::new(mailrs_fastcore::FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(store),
    ));
    let router = mailrs_fastcore::build_router(state);
    // bind synchronously so the caller has the URL before returning
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn sync_kevy_to_pg_mirrors_mail_store() {
    use mailrs_core_sync::{SyncOpts, sync};

    let kevy_base = spawn_fastcore();
    let pg_base = spawn_pg_core().await;
    let kevy = Client::new(kevy_base, String::new());
    let pg = Client::new(pg_base, String::new());
    let user = "sync-user@test";

    // seed the kevy core with an account + 2 threads x 2 messages
    kevy.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "Sync".into(),
        password: "pw".into(),
    })
    .await
    .expect("seed add_account");
    let mut uid = 1u32;
    for t in 0..2 {
        let thread = format!("th-{t}@test");
        for m in 0..2 {
            kevy.deliver_message(
                user,
                &thread,
                &deliver_req(&format!("k-{t}-{m}@test"), uid, &thread, user),
            )
            .await
            .expect("seed deliver");
            uid += 1;
        }
    }

    // run the REAL sync kevy -> pg over the contract
    let report = sync(&kevy, &pg, &SyncOpts::default())
        .await
        .expect("kevy->pg sync");
    assert_eq!(report.accounts, 1);
    assert_eq!(report.messages_delivered, 4, "all 4 messages cross to pg");
    assert_eq!(report.messages_skipped_dupe, 0);

    // assert the pg core now holds the same per-thread message-id set
    for t in 0..2 {
        let thread = format!("th-{t}@test");
        let msgs = pg
            .list_thread_messages(user, &thread)
            .await
            .expect("pg list");
        let ids: std::collections::BTreeSet<String> =
            msgs.items.iter().map(|m| m.message_id.clone()).collect();
        let expected: std::collections::BTreeSet<String> =
            (0..2).map(|m| format!("k-{t}-{m}@test")).collect();
        assert_eq!(ids, expected, "pg thread {t} mirrors kevy");
    }

    // re-run: idempotent (per-thread dedup on the pg side)
    let report2 = sync(&kevy, &pg, &SyncOpts::default())
        .await
        .expect("re-sync");
    assert_eq!(report2.messages_delivered, 0, "re-run delivers nothing new");
    assert_eq!(
        report2.messages_skipped_dupe, 4,
        "re-run skips all 4 as dupes"
    );
}

#[tokio::test]
async fn sync_pg_to_kevy_mirrors_mail_store() {
    // reverse direction: seed the PG core, sync -> kevy. Exercises the
    // pg-core's enumeration READ path (list_conversations /
    // list_thread_messages against PG) + kevy ingest, proving the
    // switch is reliable in BOTH directions across the real boundary.
    use mailrs_core_sync::{SyncOpts, sync};

    let pg_base = spawn_pg_core().await;
    let kevy_base = spawn_fastcore();
    let pg = Client::new(pg_base, String::new());
    let kevy = Client::new(kevy_base, String::new());
    let user = "rev-user@test";

    pg.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "Rev".into(),
        password: "pw".into(),
    })
    .await
    .expect("seed add_account");
    let mut uid = 1u32;
    for t in 0..2 {
        let thread = format!("rth-{t}@test");
        for m in 0..2 {
            pg.deliver_message(
                user,
                &thread,
                &deliver_req(&format!("p-{t}-{m}@test"), uid, &thread, user),
            )
            .await
            .expect("seed deliver into pg");
            uid += 1;
        }
    }

    let report = sync(&pg, &kevy, &SyncOpts::default())
        .await
        .expect("pg->kevy sync");
    assert_eq!(report.accounts, 1);
    assert_eq!(report.messages_delivered, 4, "all 4 messages cross to kevy");

    for t in 0..2 {
        let thread = format!("rth-{t}@test");
        let msgs = kevy
            .list_thread_messages(user, &thread)
            .await
            .expect("kevy list");
        let ids: std::collections::BTreeSet<String> =
            msgs.items.iter().map(|m| m.message_id.clone()).collect();
        let expected: std::collections::BTreeSet<String> =
            (0..2).map(|m| format!("p-{t}-{m}@test")).collect();
        assert_eq!(ids, expected, "kevy thread {t} mirrors pg");
    }
}

// ── contract parity on the webapi-called surface (audit point 3) ──
// fastcore and pg-core are NOT identical across the FULL contract
// (pg-core serves ~60 routes fastcore 404s). But webapi only calls a
// mail-store subset, and for THAT subset the two must be
// substitutable. This asserts semantic equality (normalized, not
// byte-identical — kevy synthesizes mailbox ids/uidvalidity) on the
// read surface after an identical seed.

async fn seed_core(c: &Client, user: &str) {
    c.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "Parity".into(),
        password: "pw".into(),
    })
    .await
    .expect("seed add_account");
    let mut uid = 1u32;
    for t in 0..3 {
        let thread = format!("pth-{t}@test");
        for m in 0..2 {
            c.deliver_message(
                user,
                &thread,
                &deliver_req(&format!("x-{t}-{m}@test"), uid, &thread, user),
            )
            .await
            .expect("seed deliver");
            uid += 1;
        }
    }
}

/// Normalized (thread_id -> sorted message-id set) for a user.
async fn thread_msg_map(c: &Client) -> std::collections::BTreeMap<String, Vec<String>> {
    use mailrs_core_api::method::conversation::ListConversationsRequest;
    use mailrs_core_api::types::ConversationFilter;
    let user = "parity@test";
    let mut out = std::collections::BTreeMap::new();
    let page = c
        .list_conversations(
            user,
            &ListConversationsRequest {
                filter: ConversationFilter {
                    limit: 100,
                    ..Default::default()
                },
            },
        )
        .await
        .expect("list_conversations");
    for s in &page.items {
        let msgs = c
            .list_thread_messages(user, &s.thread_id)
            .await
            .expect("list_thread_messages");
        let mut ids: Vec<String> = msgs.items.iter().map(|m| m.message_id.clone()).collect();
        ids.sort();
        out.insert(s.thread_id.clone(), ids);
    }
    out
}

#[tokio::test]
async fn fastcore_and_pgcore_agree_on_webapi_read_surface() {
    let user = "parity@test";
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());

    // identical seed into both cores
    seed_core(&kevy, user).await;
    seed_core(&pg, user).await;

    // both must expose the identical thread -> message-id structure
    let kmap = thread_msg_map(&kevy).await;
    let pmap = thread_msg_map(&pg).await;
    assert_eq!(
        kmap, pmap,
        "fastcore and pg-core must agree on threads+messages"
    );
    assert_eq!(kmap.len(), 3, "3 threads on both");
    for ids in kmap.values() {
        assert_eq!(ids.len(), 2, "2 messages per thread on both");
    }

    // account listing agrees on the seeded address
    let kaccts: std::collections::BTreeSet<String> = kevy
        .list_accounts()
        .await
        .expect("kevy accounts")
        .items
        .into_iter()
        .map(|a| a.address)
        .collect();
    let paccts: std::collections::BTreeSet<String> = pg
        .list_accounts()
        .await
        .expect("pg accounts")
        .items
        .into_iter()
        .map(|a| a.address)
        .collect();
    assert!(
        kaccts.contains(user) && paccts.contains(user),
        "both list the account"
    );
}

// ── SQL-core ↔ SQL-core sync (audit point 6, pg↔spg mechanism) ────
// pg and spg are the SAME PgMailboxStore code over sqlx — they differ
// only in the storage engine underneath, which sqlx abstracts. So the
// pg↔spg switch reuses mailrs-core-sync between two SQL cores. This
// runs it between two spg cores, proving a SQL-backed core works as
// BOTH sync source and destination over the contract (the exact
// read=list_conversations + write=deliver_message mechanism a real
// pg↔spg migration uses). CAVEAT: real-PostgreSQL↔real-spg with
// distinct on-disk formats + spg's held bugs remains untested.
#[tokio::test]
async fn sync_sql_core_to_sql_core() {
    use mailrs_core_sync::{SyncOpts, sync};

    let src = Client::new(spawn_pg_core().await, String::new());
    let dst = Client::new(spawn_pg_core().await, String::new());
    let user = "sql-sync@test";

    src.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "Sql".into(),
        password: "pw".into(),
    })
    .await
    .expect("seed add_account");
    let mut uid = 1u32;
    for t in 0..2 {
        let thread = format!("sth-{t}@test");
        for m in 0..2 {
            src.deliver_message(
                user,
                &thread,
                &deliver_req(&format!("s-{t}-{m}@test"), uid, &thread, user),
            )
            .await
            .expect("seed deliver");
            uid += 1;
        }
    }

    let report = sync(&src, &dst, &SyncOpts::default())
        .await
        .expect("sql->sql sync");
    assert_eq!(report.messages_delivered, 4, "all 4 cross SQL->SQL");

    for t in 0..2 {
        let thread = format!("sth-{t}@test");
        let msgs = dst
            .list_thread_messages(user, &thread)
            .await
            .expect("dst list");
        let ids: std::collections::BTreeSet<String> =
            msgs.items.iter().map(|m| m.message_id.clone()).collect();
        let expected: std::collections::BTreeSet<String> =
            (0..2).map(|m| format!("s-{t}-{m}@test")).collect();
        assert_eq!(ids, expected, "dst SQL core thread {t} mirrors src");
    }
}
