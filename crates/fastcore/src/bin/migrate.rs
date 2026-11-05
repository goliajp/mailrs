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
//! - `MAILRS_KEVY_URL` — the shared store, needed by `permissions`
//!   records: each grant or change writes an audit line there first, under
//!   the actor `host`, and a record whose line cannot be written is not
//!   imported
//!
//! Output: one progress line every 1000 records, final summary on
//! stderr. Exit code != 0 on parse failure.
//!
//! # It opens the live kevy directory, so fastcore must be stopped
//!
//! kevy 6.2.2 claims a persist directory with an advisory `flock` on
//! open, and a second engine on the same directory now **errors**.
//! Before that it interleaved: the second `Store` appended its own
//! `KEVYAOF2` magic into the first one's live AOF and the next replay
//! quarantined the record silently. A `docker exec` of this binary
//! beside a running fastcore also doubled the resident set and was
//! OOM-killed at the 1 GB limit — the visible half of the same
//! mistake.
//!
//! So: stop fastcore, run this, start it again. For anything that can
//! be done in-process, prefer a `maintenance:` route on the running
//! server, which is why most of the backfills are routes now.

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
    Account {
        blob: serde_json::Value,
    },
    Permissions {
        address: String,
        blob: serde_json::Value,
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
            account_id: String::new(),
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
            quarantined: false,
            has_action: j.has_action,
            sent_count: j.sent_count,
            starred: j.starred,
            snoozed_until: 0,
            trashed_at: 0,
        }
    }
}

/// The audit action of permissions the import grants or changes.
const PERMISSIONS_SET: &str = "account.permissions_set";

/// Write `blob` as `address`'s permissions after one audit line, actor
/// `host`, in the shared store; permissions equal to those stored are left
/// with no line. This tool makes administrators without webapi or a core
/// route, so a line that cannot be written fails the record, ungranted.
fn grant(
    mailbox: &KevyMailboxStore,
    audit: &mut Option<kevy_client::Connection>,
    address: &str,
    blob: &serde_json::Value,
) -> Result<(), String> {
    let old = mailbox
        .get_permissions_blob(address)
        .map_err(|e| format!("reading the stored permissions: {e}"))?;
    let old_value = old.as_deref().and_then(|j| serde_json::from_str(j).ok());
    if old_value.as_ref() == Some(blob) {
        return Ok(());
    }
    let new = blob.to_string();
    let detail = format!("{} -> {new}", old.as_deref().unwrap_or("none"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let conn = match audit {
        Some(c) => c,
        None => {
            let url = std::env::var("MAILRS_KEVY_URL")
                .map_err(|_| "MAILRS_KEVY_URL is needed to audit a permissions record")?;
            let c = kevy_client::Connection::connect(&url)
                .map_err(|e| format!("the shared store for the audit line: {e}"))?;
            audit.insert(c)
        }
    };
    if let Err(e) = mailrs_core_sidestate::families::audit::append(
        conn,
        "host",
        PERMISSIONS_SET,
        address,
        &detail,
        now,
    ) {
        *audit = None;
        return Err(format!(
            "the audit line was not written, nothing granted: {e}"
        ));
    }
    mailbox
        .upsert_permissions(address, &new)
        .map_err(|e| format!("upsert_permissions after its audit line: {e}"))
}

fn main() {
    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = std::sync::Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store);

    let reader = BufReader::new(std::io::stdin());
    let mut audit: Option<kevy_client::Connection> = None;
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
                // the record is this user's, so its verdict is theirs, and
                // `upsert_thread` writes none
                let written = mailbox.upsert_thread(&user, &row).and_then(|()| {
                    mailbox.set_thread_importance(
                        &user,
                        &row.thread_id,
                        &row.importance_level,
                        row.importance_score,
                    )
                });
                if let Err(e) = written {
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
                    is_own: mailrs_mailbox_kevy::senders_csv_contains_user(&wire.sender, &user),
                };
                if let Err(e) = mailbox.deliver_message(
                    &arrival,
                    &message_id,
                    &payload,
                    &mailrs_mailbox_kevy::UserMessageFacts {
                        blob_ref: &wire.blob_ref,
                        uid: wire.uid,
                        flags: wire.flags,
                        modseq: wire.modseq,
                    },
                ) {
                    eprintln!("line {idx}: deliver_message({message_id}): {e}");
                    errors += 1;
                } else {
                    messages += 1;
                }
            }
            Record::Account { blob } => {
                // Extract address from the top-level blob for the index key.
                let addr = blob
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if addr.is_empty() {
                    eprintln!("line {idx}: account blob missing address");
                    errors += 1;
                    continue;
                }
                let json = blob.to_string();
                if let Err(e) = mailbox.upsert_account(&addr, &json) {
                    eprintln!("line {idx}: upsert_account({addr}): {e}");
                    errors += 1;
                }
            }
            Record::Permissions { address, blob } => {
                if let Err(e) = grant(&mailbox, &mut audit, &address, &blob) {
                    eprintln!("line {idx}: permissions({address}): {e}");
                    errors += 1;
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
