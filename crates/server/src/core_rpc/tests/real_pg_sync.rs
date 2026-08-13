//! Cross-backend sync against REAL PostgreSQL (not in-memory spg).
//! Spins a pgvector:pg18 testcontainer, builds the pg-core over the
//! real PgPool, spawns a kevy fastcore, and runs the real
//! mailrs-core-sync BOTH directions — the highest-fidelity proof of
//! the kevy↔pg production switch axis (audit point 5, real Postgres).
//!
//! Requires docker; runs on the default (non-spg) axis with
//! `--features core-rpc`. Serialized via its own container.

// `cfg` takes one predicate, so two comma-separated ones is malformed and
// the file never compiled — under any feature combination, since a broken
// attribute is a parse error rather than a false condition. Nothing noticed
// because nothing built `--features core-rpc` after 2026-08-02.
#![cfg(all(feature = "core-rpc", not(feature = "spg")))]

// Crate-rooted rather than through `super`: this file moved down one level
// on 2026-08-02, which left `super` pointing at `core_rpc::tests` — a module
// holding only `mod` declarations. See the note in `pg_core.rs`.
use std::sync::Arc;

use crate::core_rpc::{CoreRpcState, build_full_router};

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use mailrs_core_sync::{SyncOpts, sync};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

// Anchored to the crate directory, not this file's depth — the same move
// left the old path resolving to `crates/scripts/init-schema.sql`.
const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

async fn start_pg() -> (ContainerAsync<GenericImage>, crate::pg::BackendPool) {
    // One container start at a time, workspace-wide: `cargo test
    // --workspace` runs every test binary in parallel and six fixtures
    // each start their own, which times out the wait-for-ready and
    // turns neighbours into failures. Startup only — running against a
    // live container in parallel is fine.
    let _startup = mailrs_test_docker::startup_lock().await;
    let container = GenericImage::new("pgvector/pgvector", "pg18")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_exposed_port(5432.tcp())
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "mailrs_test")
        .with_env_var("POSTGRES_USER", "postgres")
        .start()
        .await
        .expect("start pgvector");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:test@{host}:{port}/mailrs_test");
    // race the listener readiness
    let pool = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match sqlx::PgPool::connect(&url).await {
                Ok(p) => break p,
                Err(_) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                Err(e) => panic!("pg never came up: {e}"),
            }
        }
    };
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply schema");
    sqlx::query("INSERT INTO domains (name) VALUES ('test') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();
    (container, pool)
}

fn spawn_pg_core(pool: crate::pg::BackendPool) -> String {
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
        maildir_root: "/tmp/real-pg-core-test".into(),
        net_url: None,
    });
    let router = build_full_router(state, String::new());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let l = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(l, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn spawn_fastcore() -> String {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(store),
    ));
    let router = mailrs_fastcore::build_router(state);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let l = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(l, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn deliver_req(mid: &str, uid: u32, thread: &str, user: &str) -> DeliverMessageRequest {
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": format!("{mid}.host"),
        "sender": "remote@x.y", "recipients": user, "subject": "Hi",
        "date": 1_700_000_000i64, "internal_date": 1_700_000_000i64,
        "size": 42, "flags": 1, "message_id": mid, "in_reply_to": "",
        "thread_id": thread, "modseq": 0, "user_address": user,
    });
    DeliverMessageRequest {
        message_id: mid.into(),
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

async fn thread_ids(
    c: &Client,
    user: &str,
    threads: usize,
) -> Vec<std::collections::BTreeSet<String>> {
    let mut out = Vec::new();
    for t in 0..threads {
        let thread = format!("rp-{t}@test");
        let msgs = c.list_thread_messages(user, &thread).await.expect("list");
        out.push(msgs.items.iter().map(|m| m.message_id.clone()).collect());
    }
    out
}

#[tokio::test]
async fn real_pg_bidirectional_sync() {
    let (_container, pool) = start_pg().await;
    let pg = Client::new(spawn_pg_core(pool), String::new());
    let kevy = Client::new(spawn_fastcore(), String::new());
    let user = "realpg@test";

    // seed the kevy core
    kevy.add_account(&AddAccountRequest {
        address: user.into(),
        display_name: "Real".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");
    let mut uid = 1u32;
    for t in 0..2 {
        let thread = format!("rp-{t}@test");
        for m in 0..2 {
            kevy.deliver_message(
                user,
                &thread,
                &deliver_req(&format!("rp-{t}-{m}@test"), uid, &thread, user),
            )
            .await
            .expect("seed deliver");
            uid += 1;
        }
    }

    // kevy -> REAL pg
    let r = sync(&kevy, &pg, &SyncOpts::default())
        .await
        .expect("kevy->pg");
    assert_eq!(r.messages_delivered, 4, "4 msgs land in real Postgres");
    let expected: Vec<std::collections::BTreeSet<String>> = (0..2)
        .map(|t| (0..2).map(|m| format!("rp-{t}-{m}@test")).collect())
        .collect();
    assert_eq!(
        thread_ids(&pg, user, 2).await,
        expected,
        "real pg mirrors kevy"
    );

    // re-run idempotent against real pg (find_by_message_id guard)
    let r2 = sync(&kevy, &pg, &SyncOpts::default())
        .await
        .expect("re-sync");
    assert_eq!(r2.messages_delivered, 0, "real-pg re-run delivers nothing");

    // reverse: REAL pg -> a fresh kevy
    let kevy2 = Client::new(spawn_fastcore(), String::new());
    let rr = sync(&pg, &kevy2, &SyncOpts::default())
        .await
        .expect("pg->kevy");
    assert_eq!(rr.messages_delivered, 4, "4 msgs cross back from real pg");
    assert_eq!(
        thread_ids(&kevy2, user, 2).await,
        expected,
        "kevy2 mirrors real pg"
    );
}
