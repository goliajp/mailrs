//! Bidirectional sync round-trip: two live fastcore instances over real
//! HTTP. Validates the whole `mailrs-core-sync` path + the contract
//! routes (list_accounts / list_conversations / list_thread_messages /
//! deliver_message / add_account / aliases) end to end, plus the
//! idempotency guard (re-running sync must not duplicate).
//!
//! Both cores are kevy here (in-memory) so the test needs no docker; the
//! sync tool is backend-blind, so a kevy↔kevy round-trip exercises every
//! line the kevy↔pg switch does except the PG store internals (covered
//! by the PG smoke suite).

use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use mailrs_core_sync::{SyncOpts, sync};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::KevyMailboxStore;

/// Spawn an in-memory fastcore on an ephemeral port; return its base URL.
async fn spawn_core() -> String {
    let store = Arc::new(Store::open(Config::default()).unwrap());
    let state = Arc::new(FastcoreState::new(KevyMailboxStore::new(store)));
    let router = mailrs_fastcore::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn client(base: &str) -> Client {
    // fastcore's build_router has no auth layer — any secret works.
    Client::new(base.to_string(), String::new())
}

/// Seed a core with one account and `n_threads` × `n_msgs` messages.
async fn seed(c: &Client, user: &str, n_threads: usize, n_msgs: usize) {
    c.add_account(&AddAccountRequest {
        address: user.to_string(),
        display_name: "Seed User".into(),
        password: "seed-pw".into(),
    })
    .await
    .unwrap();

    let mut uid = 1u32;
    for t in 0..n_threads {
        let thread_id = format!("thread-{t}@test");
        for m in 0..n_msgs {
            let message_id = format!("msg-{t}-{m}@test");
            let ts = 1_700_000_000i64 + (t * 100 + m) as i64;
            let wire = serde_json::json!({
                "id": 0,
                "mailbox_id": 0,
                "uid": uid,
                "blob_ref": format!("{ts}.M{uid}.host"),
                "sender": "someone@remote.test",
                "recipients": user,
                "subject": format!("Thread {t}"),
                "date": ts,
                "internal_date": ts,
                "size": 100,
                "flags": 0,
                "message_id": message_id,
                "in_reply_to": if m == 0 { String::new() } else { format!("msg-{t}-{}@test", m - 1) },
                "thread_id": thread_id,
                "modseq": 0,
                "user_address": user,
            });
            let req = DeliverMessageRequest {
                message_id: message_id.clone(),
                subject: format!("Thread {t}"),
                senders_csv: "someone@remote.test".into(),
                latest_date: ts,
                latest_preview: String::new(),
                category: "inbox".into(),
                unread: true,
                uid,
                payload_wire_json: wire.to_string(),
            };
            c.deliver_message(user, &thread_id, &req).await.unwrap();
            uid += 1;
        }
    }
}

/// The full set of (thread_id, message_id) a core holds for `user`,
/// enumerated over the contract exactly like sync reads.
async fn snapshot(c: &Client, user: &str) -> std::collections::BTreeSet<String> {
    use mailrs_core_api::method::conversation::ListConversationsRequest;
    use mailrs_core_api::types::ConversationFilter;
    let mut out = std::collections::BTreeSet::new();
    for archived in [false, true] {
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: 500,
                archived,
                ..Default::default()
            },
        };
        let page = c.list_conversations(user, &req).await.unwrap();
        for s in &page.items {
            let msgs = c.list_thread_messages(user, &s.thread_id).await.unwrap();
            for m in &msgs.items {
                out.insert(format!("{}::{}", s.thread_id, m.message_id));
            }
        }
    }
    out
}

#[tokio::test]
async fn sync_mirrors_and_is_idempotent() {
    let base_a = spawn_core().await;
    let base_b = spawn_core().await;
    let (a, b) = (client(&base_a), client(&base_b));
    let user = "user@test";

    seed(&a, user, 3, 2).await;
    let snap_a = snapshot(&a, user).await;
    assert_eq!(snap_a.len(), 6, "seed should have 3 threads × 2 messages");

    // A → B: B must mirror A
    let r1 = sync(&a, &b, &SyncOpts::default()).await.unwrap();
    assert_eq!(r1.accounts, 1);
    assert_eq!(r1.messages_delivered, 6);
    assert_eq!(r1.messages_skipped_dupe, 0);
    assert_eq!(snapshot(&b, user).await, snap_a, "B mirrors A after sync");

    // A → B again: stable, no duplicates (guards kevy's non-idempotent
    // thread counters — everything is skipped as already present)
    let r2 = sync(&a, &b, &SyncOpts::default()).await.unwrap();
    assert_eq!(r2.messages_delivered, 0, "re-run delivers nothing new");
    assert_eq!(r2.messages_skipped_dupe, 6, "re-run skips all as dupes");
    assert_eq!(snapshot(&b, user).await, snap_a, "B unchanged on re-run");

    // B → A reverse: A already has everything, so it's a stable no-op
    let r3 = sync(&b, &a, &SyncOpts::default()).await.unwrap();
    assert_eq!(
        r3.messages_delivered, 0,
        "reverse into full A delivers nothing"
    );
    assert_eq!(
        snapshot(&a, user).await,
        snap_a,
        "A unchanged after reverse"
    );
}
