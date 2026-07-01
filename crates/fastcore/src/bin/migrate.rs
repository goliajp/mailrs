//! `mailrs-fastcore-migrate` — JSONL-driven import into the kevy
//! backend. Phase 10 (small scope).
//!
//! Reads NDJSON from stdin, one record per line, in this shape:
//!
//! ```text
//!   {"kind":"thread","user":"u@x.com","row":{ThreadRow JSON}}
//!   {"kind":"message","user":"u@x.com","thread_id":"t1","message_id":"m1",
//!    "internal_date":1700000000,"wire":{MessageWire JSON}}
//! ```
//!
//! - `thread` records call `upsert_thread`
//! - `message` records call `deliver_message`
//!
//! Decoupling the import from PG schema specifics: any operator can
//! produce this JSONL via `psql --csv` + a one-liner jq or a future
//! `mailrs-server dump` subcommand. This binary stays simple and the
//! migration step is testable in isolation.
//!
//! Env:
//! - `MAILRS_KEVY_DATA_DIR` — same as fastcore proper, default
//!   `/data/kevy-fastcore`
//!
//! Output: one progress line every 1000 records, final summary on
//! stderr. Exit code != 0 on parse failure.

use std::io::{BufRead, BufReader};

use kevy_embedded::{Config, Store};
use mailrs_core_api::method::message::MessageWire;
use mailrs_mailbox_kevy::{KevyMailboxStore, MessageArrival, ThreadRow};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record {
    Thread {
        user: String,
        row: ThreadRowJson,
    },
    Message {
        #[allow(dead_code)]
        user: String,
        thread_id: String,
        message_id: String,
        internal_date: i64,
        category: Option<String>,
        unread: Option<bool>,
        wire: MessageWire,
    },
}

#[derive(Debug, Deserialize)]
struct ThreadRowJson {
    thread_id: String,
    subject: String,
    senders_csv: String,
    count: i64,
    unread_count: i64,
    latest_date: i64,
    latest_preview: String,
    category: String,
    importance_level: String,
    importance_score: f64,
    requires_action: bool,
    pinned: bool,
    archived: bool,
    has_action: bool,
    sent_count: i64,
    #[serde(default)]
    starred: bool,
}

impl From<ThreadRowJson> for ThreadRow {
    fn from(j: ThreadRowJson) -> Self {
        ThreadRow {
            thread_id: j.thread_id,
            subject: j.subject,
            senders_csv: j.senders_csv,
            count: j.count,
            unread_count: j.unread_count,
            latest_date: j.latest_date,
            latest_preview: j.latest_preview,
            category: j.category,
            importance_level: j.importance_level,
            importance_score: j.importance_score,
            requires_action: j.requires_action,
            pinned: j.pinned,
            archived: j.archived,
            has_action: j.has_action,
            sent_count: j.sent_count,
            starred: j.starred,
        }
    }
}

fn main() {
    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = std::sync::Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store);

    let reader = BufReader::new(std::io::stdin());
    let mut threads = 0u64;
    let mut messages = 0u64;
    let mut errors = 0u64;

    for (idx, line) in reader.lines().enumerate() {
        let Ok(line) = line else {
            eprintln!("line {idx}: read failed");
            errors += 1;
            continue;
        };
        let line = line.trim();
        // Skip empty, comments (#), and pg-dump progress lines that
        // leaked in via `2>&1`. Any line that isn't a JSON object
        // start is a diagnostic — don't count as a parse error.
        if line.is_empty() || line.starts_with('#') || !line.starts_with('{') {
            continue;
        }
        let Ok(rec): Result<Record, _> = serde_json::from_str(line) else {
            eprintln!("line {idx}: parse failed: {line}");
            errors += 1;
            continue;
        };
        match rec {
            Record::Thread { user, row } => {
                let row: ThreadRow = row.into();
                if let Err(e) = mailbox.upsert_thread(&user, &row) {
                    eprintln!("line {idx}: upsert_thread({}): {e}", row.thread_id);
                    errors += 1;
                } else {
                    threads += 1;
                }
            }
            Record::Message {
                user,
                thread_id,
                message_id,
                internal_date,
                category,
                unread,
                wire,
            } => {
                let payload = match serde_json::to_vec(&wire) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("line {idx}: serialize wire: {e}");
                        errors += 1;
                        continue;
                    }
                };
                let cat = category.unwrap_or_else(|| "inbox".to_string());
                let arrival = MessageArrival {
                    thread_id: &thread_id,
                    user: &user,
                    subject: &wire.subject,
                    senders_csv: &wire.sender,
                    latest_date: internal_date,
                    latest_preview: "",
                    category: &cat,
                    unread: unread.unwrap_or(false),
                };
                if let Err(e) = mailbox.deliver_message(&arrival, &message_id, &payload) {
                    eprintln!("line {idx}: deliver_message({message_id}): {e}");
                    errors += 1;
                } else {
                    messages += 1;
                }
            }
        }
        let n = threads + messages;
        if n > 0 && n.is_multiple_of(1000) {
            eprintln!("progress: {threads} threads, {messages} messages, {errors} errors");
        }
    }
    eprintln!("done: {threads} threads, {messages} messages, {errors} errors");
    if errors > 0 {
        std::process::exit(2);
    }
}
