//! `mailrs-pg-dump` — Walk the PG (or spg) catalog and emit NDJSON
//! suitable for `mailrs-fastcore-migrate`. Phase 10b — bootstraps a
//! fresh kevy backend from the existing spg source-of-truth so the
//! cutover preserves all conversation + message history.
//!
//! Wire format (one record per line):
//! ```text
//!   {"kind":"thread","user":"u@x.com","row":{ThreadRow JSON}}
//!   {"kind":"message","user":"u@x.com","thread_id":"t1","message_id":"m1",
//!    "internal_date":1700000000,"wire":{MessageWire JSON}}
//! ```
//!
//! Usage:
//! ```bash
//!   MAILRS_PG_URL=spg:///data/spg/mailrs.spg \
//!       mailrs-pg-dump --user u@x.com > dump.ndjson
//!   # ... pipes into:
//!   cat dump.ndjson | mailrs-fastcore-migrate
//! ```
//!
//! Args:
//! - `--user <addr>` — dump only one account (default: every account)
//! - `--since <epoch-seconds>` — skip messages older than the cutoff
//! - `--limit <N>` — cap threads per account (debug aid; 0 = unlimited)
//!
//! Output: NDJSON on stdout. Progress on stderr (every 100 threads).

use std::collections::HashSet;

use mailrs_core_api::method::message::MessageWire;
use mailrs_mailbox::PgMailboxStore;
use mailrs_mailbox::store::MailboxStore;
use serde::Serialize;

#[derive(Serialize)]
struct ThreadRowJson<'a> {
    thread_id: &'a str,
    subject: &'a str,
    senders_csv: &'a str,
    count: i64,
    unread_count: i64,
    latest_date: i64,
    latest_preview: &'a str,
    category: &'a str,
    importance_level: &'a str,
    importance_score: f64,
    requires_action: bool,
    pinned: bool,
    archived: bool,
    has_action: bool,
    sent_count: i64,
    starred: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OutRecord<'a> {
    Thread {
        user: &'a str,
        row: ThreadRowJson<'a>,
    },
    Message {
        user: &'a str,
        thread_id: &'a str,
        message_id: &'a str,
        internal_date: i64,
        wire: &'a MessageWire,
    },
}

fn parse_args() -> (Option<String>, Option<i64>, u32) {
    let mut user: Option<String> = None;
    let mut since: Option<i64> = None;
    let mut limit: u32 = 0;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--user" => user = it.next(),
            "--since" => since = it.next().and_then(|s| s.parse().ok()),
            "--limit" => limit = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "-h" | "--help" => {
                eprintln!("usage: mailrs-pg-dump [--user <addr>] [--since <epoch>] [--limit <N>]");
                eprintln!("env:   MAILRS_PG_URL — spg:// or postgres:// URL (required)");
                std::process::exit(0);
            }
            _ => {
                eprintln!("unknown arg: {a}");
                std::process::exit(2);
            }
        }
    }
    (user, since, limit)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let (user_arg, since, limit) = parse_args();
    let pg_url = std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required");
    eprintln!("connecting to {pg_url} ...");
    #[cfg(feature = "spg")]
    let pool = spg_sqlx::SpgPoolOptions::new()
        .max_connections(4)
        .connect(&pg_url)
        .await
        .expect("connect spg");
    #[cfg(not(feature = "spg"))]
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&pg_url)
        .await
        .expect("connect pg");
    let store = PgMailboxStore::new(pool.clone());

    // 1. Collect users to dump
    let users: Vec<String> = if let Some(u) = user_arg {
        vec![u]
    } else {
        // Fall back to scanning the conversations view for distinct user addrs.
        // Avoids a hard dep on DomainStore (which is server-side).
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT user_address FROM messages WHERE user_address <> ''",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    };
    eprintln!("dumping {} user(s)", users.len());

    let mut thread_count = 0u64;
    let mut message_count = 0u64;

    for user in &users {
        // 2. Each user: list conversations (paged via before_ts cursor)
        let page_size: u32 = if limit == 0 { 200 } else { limit.min(200) };
        let mut before_ts: Option<i64> = None;
        let mut seen_tids: HashSet<String> = HashSet::new();
        loop {
            let summaries = match store
                .list_conversations(
                    user, page_size, before_ts, None, None, false, None, None, None, None,
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("list_conversations({user}) failed: {e}");
                    break;
                }
            };
            if summaries.is_empty() {
                break;
            }
            for s in &summaries {
                if seen_tids.contains(&s.thread_id) {
                    continue;
                }
                seen_tids.insert(s.thread_id.clone());
                // emit thread row
                let row = ThreadRowJson {
                    thread_id: &s.thread_id,
                    subject: &s.subject,
                    senders_csv: &s.participants,
                    count: s.message_count as i64,
                    unread_count: s.unread_count as i64,
                    latest_date: s.last_date,
                    latest_preview: &s.snippet,
                    category: &s.category,
                    importance_level: &s.importance_level,
                    importance_score: s.importance_score as f64,
                    requires_action: s.requires_action,
                    pinned: s.pinned,
                    archived: s.archived,
                    has_action: s.requires_action,
                    sent_count: s.sent_count as i64,
                    starred: s.flagged,
                };
                println!(
                    "{}",
                    serde_json::to_string(&OutRecord::Thread { user, row }).unwrap()
                );
                thread_count += 1;

                // emit each message in the thread
                let mids = match <PgMailboxStore as MailboxStore>::thread_message_ids(
                    &store,
                    user,
                    &s.thread_id,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("thread_message_ids({user},{}) failed: {e}", s.thread_id);
                        continue;
                    }
                };
                for mid in mids {
                    // Fetch message by store-native id; we only need a
                    // small slice for fastcore.
                    let m = match store.get_message_by_db_id(user, mid).await {
                        Ok(Some(m)) => m,
                        _ => continue,
                    };
                    if let Some(since) = since
                        && m.internal_date < since
                    {
                        continue;
                    }
                    let wire = MessageWire::from(&m);
                    println!(
                        "{}",
                        serde_json::to_string(&OutRecord::Message {
                            user,
                            thread_id: &s.thread_id,
                            message_id: &wire.message_id,
                            internal_date: wire.internal_date,
                            wire: &wire,
                        })
                        .unwrap()
                    );
                    message_count += 1;
                }
            }
            // next page: before_ts = oldest of this page
            let oldest = summaries.iter().map(|s| s.last_date).min().unwrap_or(0);
            before_ts = Some(oldest);
            if thread_count.is_multiple_of(100) && thread_count > 0 {
                eprintln!("progress: {thread_count} threads, {message_count} messages");
            }
            if (summaries.len() as u32) < page_size {
                break;
            }
        }
    }
    eprintln!("done: {thread_count} threads, {message_count} messages");
}
