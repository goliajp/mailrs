//! Dispatcher benchmarks: drive each of the 7 dispatched JMAP methods plus the
//! `dispatch_request` envelope (including a multi-call with back-reference)
//! against a minimal in-memory `MailStore` impl.
//!
//! The in-memory store here is intentionally simpler than the integration
//! fixture in `tests/common/mod.rs` — no error injection, no builder API. It
//! just answers every call with canned data so the benchmarks isolate
//! dispatcher overhead.

use std::hint::black_box;

use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::{Value, json};

use mailrs_jmap::dispatch::{JmapRequest, dispatch_method, dispatch_request};
use mailrs_jmap::store::{MailStore, StoreError};
use mailrs_jmap::types::{
    FLAG_ANSWERED, FLAG_SEEN, Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult,
};

const TEST_USER: &str = "alice@example.com";

struct BenchStore {
    mailboxes: Vec<Mailbox>,
    messages: Vec<Message>,
    raw: Vec<u8>,
}

impl BenchStore {
    fn new() -> Self {
        Self {
            mailboxes: vec![
                Mailbox {
                    id: 1,
                    name: "INBOX".into(),
                },
                Mailbox {
                    id: 2,
                    name: "Sent".into(),
                },
            ],
            messages: (1..=10).map(make_message).collect(),
            raw: b"From: x\r\n\r\nbody payload".to_vec(),
        }
    }
}

fn make_message(i: i64) -> Message {
    Message {
        id: i,
        mailbox_id: 1,
        uid: i as u32,
        sender: "Sender <sender@example.com>".into(),
        recipients: TEST_USER.into(),
        subject: format!("message {i}"),
        date: 1_700_000_000 + i,
        size: 256,
        flags: if i % 2 == 0 {
            FLAG_SEEN | FLAG_ANSWERED
        } else {
            0
        },
        internal_date: 1_700_000_000 + i,
        message_id: format!("msg-{i}@example.com"),
        in_reply_to: String::new(),
        thread_id: "thread-A".into(),
        user_address: TEST_USER.into(),
        new_content: Some(format!("preview {i}")),
        blob_id: format!("blob-{i}"),
    }
}

#[async_trait]
impl MailStore for BenchStore {
    async fn list_mailboxes(&self, _user: &str) -> Result<Vec<Mailbox>, StoreError> {
        Ok(self.mailboxes.clone())
    }

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxCounts, StoreError> {
        let total = self
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id)
            .count() as u32;
        let unread = self
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id && m.flags & FLAG_SEEN == 0)
            .count() as u32;
        Ok(MailboxCounts { total, unread })
    }

    async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Message>, StoreError> {
        Ok(self
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id)
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn get_message_by_db_id(
        &self,
        user: &str,
        id: i64,
    ) -> Result<Option<Message>, StoreError> {
        Ok(self
            .messages
            .iter()
            .find(|m| m.id == id && m.user_address == user)
            .cloned())
    }

    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<Message>, StoreError> {
        Ok(self
            .messages
            .iter()
            .filter(|m| m.thread_id == thread_id && m.user_address == user)
            .cloned()
            .collect())
    }

    async fn update_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> {
        Ok(())
    }

    async fn add_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> {
        Ok(())
    }

    async fn read_message_raw(&self, _: &Message) -> Option<Vec<u8>> {
        Some(self.raw.clone())
    }

    fn parse_message(&self, _: &[u8]) -> ParsedBody {
        ParsedBody {
            text: Some("body payload".into()),
            html: None,
            attachments: vec![],
        }
    }

    async fn submit_message(&self, _: &str, _: &Message, _: &[u8]) -> SubmissionResult {
        SubmissionResult {
            success: true,
            message: None,
        }
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn bench_dispatch_methods(c: &mut Criterion) {
    let rt = rt();
    let store = BenchStore::new();

    // Mailbox/get — list all mailboxes + per-mailbox status
    let args_mailbox_get = json!({});
    c.bench_function("dispatch_mailbox_get", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Mailbox/get"),
                    black_box(&args_mailbox_get),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Mailbox/query
    c.bench_function("dispatch_mailbox_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Mailbox/query"),
                    black_box(&args_mailbox_get),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Email/get — single message, metadata only (no body read)
    let args_email_get_meta = json!({
        "ids": ["msg-1"],
        "properties": ["subject", "from", "receivedAt"]
    });
    c.bench_function("dispatch_email_get_meta_only", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Email/get"),
                    black_box(&args_email_get_meta),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Email/get with body (raw read + parse + body fields)
    let args_email_get_full = json!({
        "ids": ["msg-1"],
        "properties": ["bodyValues", "textBody", "subject"]
    });
    c.bench_function("dispatch_email_get_with_body", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Email/get"),
                    black_box(&args_email_get_full),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Email/query — sort by receivedAt desc across 10 messages
    let args_email_query = json!({"limit": 50});
    c.bench_function("dispatch_email_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Email/query"),
                    black_box(&args_email_query),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Email/set — update keywords on one message
    let args_email_set = json!({
        "update": {"msg-1": {"keywords/$seen": true}}
    });
    c.bench_function("dispatch_email_set_update", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Email/set"),
                    black_box(&args_email_set),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // Thread/get — return all messages in a thread
    let args_thread_get = json!({"ids": ["thread-A"]});
    c.bench_function("dispatch_thread_get", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("Thread/get"),
                    black_box(&args_thread_get),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });

    // EmailSubmission/set — submit one previously-stored message
    let args_submit = json!({
        "create": {"k1": {"emailId": "msg-1"}}
    });
    c.bench_function("dispatch_email_submission_set", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_method(
                    black_box("EmailSubmission/set"),
                    black_box(&args_submit),
                    black_box(TEST_USER),
                    &store,
                )
                .await
            })
        })
    });
}

fn bench_envelope(c: &mut Criterion) {
    let rt = rt();
    let store = BenchStore::new();

    // Single-call envelope (dispatcher + serialization overhead with one method)
    let single_call: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Mailbox/get", {}, "c1"]
        ]
    }))
    .unwrap();
    c.bench_function("dispatch_request_single_call", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_request(black_box(single_call.clone()), TEST_USER, &store).await
            })
        })
    });

    // Multi-call envelope with a back-reference — the canonical
    // "Email/query → Email/get with #ids" pattern that drove the back-reference
    // feature into the spec in the first place.
    let multi_call_with_backref_value: Value = json!({
        "using": ["urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/query", {"limit": 10}, "c1"],
            ["Email/get", {
                "#ids": {"resultOf": "c1", "name": "Email/query", "path": "/ids"},
                "properties": ["subject", "from"]
            }, "c2"]
        ]
    });
    let multi_call_with_backref: JmapRequest =
        serde_json::from_value(multi_call_with_backref_value).unwrap();
    c.bench_function("dispatch_request_multi_call_back_ref", |b| {
        b.iter(|| {
            rt.block_on(async {
                dispatch_request(
                    black_box(multi_call_with_backref.clone()),
                    TEST_USER,
                    &store,
                )
                .await
            })
        })
    });
}

criterion_group!(dispatch_benches, bench_dispatch_methods, bench_envelope);
criterion_main!(dispatch_benches);
