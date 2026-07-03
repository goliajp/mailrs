//! `mailrs-fastcore` — Kevy-backed implementation of the
//! `mailrs-core-api` server surface. Phase 8.
//!
//! Today this binary mounts a small subset:
//! - `/v1/healthz` + `/v1/readyz` (open) — proves the role works
//! - `POST /v1/users/{user}/conversations:list` — Rock 1 read path
//!
//! The rest of the 87-route surface fills in as `mailbox-kevy` grows
//! method coverage. Run alongside (or instead of) the monolith core
//! to A/B test conversation-list latency under the same load.
//!
//! Environment:
//! - `MAILRS_FASTCORE_BIND` — listen address (default `0.0.0.0:3301`,
//!   one above the monolith's core-rpc :3300 so both can coexist)
//! - `MAILRS_KEVY_DATA_DIR` — kevy persist dir (default
//!   `/data/kevy-fastcore`)

#![allow(missing_docs)]

mod acme_task;
mod imap;
mod live_sync;
mod pop3;
mod sieve_apply;
mod spool_drain;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use kevy_embedded::{Config, Store};
use mailrs_core_api::method::admin as adm;
use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::mailbox as mb;
use mailrs_core_api::method::message as msg;
use mailrs_core_api::method::thread as th;
use mailrs_core_api::server::{Handler, base_router};
use mailrs_core_api::types::{BackendKind, ConversationSummaryWire, HealthResponse};
use mailrs_mailbox_kevy::{KevyMailboxStore, ListThreadsFilter, ThreadRow};

/// Server state — owns the kevy store and is cloned into axum handlers.
pub struct FastcoreState {
    pub mailbox: KevyMailboxStore,
}

impl Handler for FastcoreState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: true,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // kevy is in-process; if the binary is up, the store is up.
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: true,
        }
    }
}

pub async fn run() {
    // Install the process-wide rustls crypto provider before any TLS
    // config is built (IMAPS / POP3S acceptors, ACME challenge server).
    // Without this rustls 0.23 panics on first use — same fix
    // mailrs-receiver / mailrs-fastcore-sender apply. `.ok()` because
    // a second install is a no-op error we can safely ignore.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store);
    let state = Arc::new(FastcoreState { mailbox });

    // Spawn the ingestion sync loop before the HTTP listener so new
    // messages start replicating as soon as the process boots. Failures
    // are logged + retried; they don't crash the server.
    let sync_state = state.clone();
    tokio::spawn(async move {
        ingest_sync_loop(sync_state).await;
    });

    // Spawn the spool drain — receiver writes {spool}/incoming/new/*
    // in split topology, and nothing else consumes it. Missing this
    // task is what causes inbound Gmail / GitHub / etc. to sit in the
    // spool forever ("Sender said 250 OK, user never sees it"). See
    // `spool_drain.rs`.
    let drain_state = state.clone();
    tokio::spawn(async move {
        spool_drain::spawn(drain_state).await;
    });

    // ACME renewal task. Reads MAILRS_ACME_EMAIL/DOMAINS; noop if
    // either is unset. Binds port 80 for the HTTP-01 challenge server
    // and periodically renews the cert to `MAILRS_ACME_DIR`. Receiver
    // + webapi consume the resulting cert files on their own reload
    // cadence — fastcore doesn't serve TLS itself.
    tokio::spawn(async move {
        acme_task::spawn().await;
    });

    // IMAP + IMAPS + POP3 + POP3S listeners. Cert comes from
    // MAILRS_TLS_CERT + MAILRS_TLS_KEY (same paths the receiver uses)
    // — matching the monolith's TLS pattern: plaintext port loads no
    // cert, implicit-TLS port wraps every accepted socket via a
    // shared rustls acceptor before entering the session. Set each
    // MAILRS_(IMAP|IMAPS|POP3|POP3S)_BIND=off to disable per-port.
    let imap_state = state.clone();
    tokio::spawn(async move {
        imap::spawn(imap_state).await;
    });
    let imaps_state = state.clone();
    tokio::spawn(async move {
        imap::spawn_tls(imaps_state).await;
    });
    let pop3_state = state.clone();
    tokio::spawn(async move {
        pop3::spawn(pop3_state).await;
    });
    let pop3s_state = state.clone();
    tokio::spawn(async move {
        pop3::spawn_tls(pop3s_state).await;
    });

    let addr = std::env::var("MAILRS_FASTCORE_BIND").unwrap_or_else(|_| "0.0.0.0:3301".into());

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind MAILRS_FASTCORE_BIND");
    tracing::info!(
        addr = %addr,
        kevy_dir = %kevy_dir,
        "mailrs-fastcore listening (kevy backend)"
    );
    axum::serve(listener, app).await.unwrap();
}

/// Poll monolith's core-rpc every 30 s for threads newer than the last
/// synced timestamp per user; upsert them into kevy so mail delivered
/// after cutover shows up in the fastcore-served inbox without a manual
/// re-migrate. Uses monolith solely as the source of "what's arrived
/// since the last poll" — user-visible reads never touch it.
async fn ingest_sync_loop(state: Arc<FastcoreState>) {
    let Ok(base) = std::env::var("MAILRS_CORE_RPC_BASE") else {
        tracing::warn!("MAILRS_CORE_RPC_BASE unset — ingestion loop disabled");
        return;
    };
    let Ok(secret) = std::env::var(mailrs_core_api::AUTH_SECRET_ENV) else {
        tracing::warn!("MAILRS_CORE_API_SECRET unset — ingestion loop disabled");
        return;
    };
    let client = mailrs_core_api::client::Client::new(base, secret);
    let interval = std::time::Duration::from_secs(
        std::env::var("MAILRS_FASTCORE_SYNC_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30),
    );
    loop {
        if let Err(e) = run_ingest_once(&state, &client).await {
            tracing::warn!(error = %e, "ingest sync tick failed");
        }
        tokio::time::sleep(interval).await;
    }
}

async fn run_ingest_once(
    state: &Arc<FastcoreState>,
    client: &mailrs_core_api::client::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    use mailrs_core_api::method::conversation::ListConversationsRequest;
    use mailrs_core_api::types::ConversationFilter;

    let addrs = state.mailbox.list_account_addresses()?;
    for user in &addrs {
        let cursor_key = format!("mailrs:sync:cursor:{user}");
        let prev = state
            .mailbox
            .store_ref()
            .get(cursor_key.as_bytes())?
            .and_then(|b| String::from_utf8_lossy(&b).parse::<i64>().ok())
            .unwrap_or(0);
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: 200,
                before_ts: None,
                category: None,
                domains: None,
                archived: false,
                folder: None,
                unread: None,
                starred: None,
                section: None,
            },
        };
        // Try monolith. If it's down, skip the ingest step but STILL
        // run the maildir-based self-heal at the bottom of the loop —
        // fastcore's whole point is to work without monolith.
        let resp_opt = match client.list_conversations(user, &req).await {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(error = %e, %user, "monolith list_conversations failed (continuing to self-heal from maildir)");
                None
            }
        };
        let resp = match resp_opt {
            Some(r) => r,
            None => {
                healed_from_maildir(state, user).await;
                continue;
            }
        };
        let mut max_seen = prev;
        let mut newly = 0;
        for s in &resp.items {
            if s.last_date <= prev {
                continue;
            }
            // If the thread already exists in kevy, don't clobber the
            // aggregate (fastcore-side mark_read / pin / archive stay
            // sticky) — but DO diff messages, because a thread with a
            // new reply gets its `last_date` bumped and needs the new
            // message body ingested. Prior version skipped the whole
            // packet, so new replies never appeared until the user
            // re-imported.
            let already_exists = matches!(state.mailbox.get_thread(&s.thread_id), Ok(Some(_)));
            if already_exists {
                if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                    for w in &msgs.items {
                        // Only write if we don't already have this
                        // message id (prevents duplicate writes on
                        // every sync tick).
                        if state
                            .mailbox
                            .get_message(&w.message_id)
                            .ok()
                            .flatten()
                            .is_some()
                        {
                            continue;
                        }
                        let payload = match serde_json::to_vec(w) {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        let _ = state.mailbox.upsert_message(
                            &s.thread_id,
                            &w.message_id,
                            w.internal_date,
                            &payload,
                        );
                        let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                    }
                }
                max_seen = max_seen.max(s.last_date);
                continue;
            }
            let row = mailrs_mailbox_kevy::ThreadRow {
                thread_id: s.thread_id.clone(),
                subject: s.subject.clone(),
                senders_csv: s.participants.clone(),
                count: s.message_count as i64,
                unread_count: s.unread_count as i64,
                latest_date: s.last_date,
                latest_preview: s.snippet.clone(),
                category: s.category.clone(),
                importance_level: s.importance_level.clone(),
                importance_score: s.importance_score as f64,
                requires_action: s.requires_action,
                pinned: s.pinned,
                archived: s.archived,
                has_action: s.requires_action,
                sent_count: s.sent_count as i64,
                starred: s.flagged,
            };
            if let Err(e) = state.mailbox.upsert_thread(user, &row) {
                tracing::warn!(error = %e, %user, tid = %s.thread_id, "upsert_thread failed");
                continue;
            }
            // Pull the thread's messages and mirror them so `get_thread_messages`
            // returns the fresh content on the next click.
            if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                for w in &msgs.items {
                    let payload = match serde_json::to_vec(w) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let _ = state.mailbox.upsert_message(
                        &s.thread_id,
                        &w.message_id,
                        w.internal_date,
                        &payload,
                    );
                    let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                }
            }
            max_seen = max_seen.max(s.last_date);
            newly += 1;
        }
        if newly > 0 {
            state
                .mailbox
                .store_ref()
                .set(cursor_key.as_bytes(), max_seen.to_string().as_bytes())?;
            tracing::info!(%user, newly, cursor = max_seen, "ingest sync applied");
        }

        // Self-heal pass — reads maildir directly, no monolith call.
        //
        // Fastcore's whole reason for existing is to be spg-independent.
        // If we heal by calling monolith, then a spg outage takes
        // fastcore down with it — defeats the point. Instead, walk the
        // user's maildir(s), parse each file's headers, and upsert any
        // messages whose thread_id already exists in fastcore but has
        // an empty messages zset.
        healed_from_maildir(state, user).await;
    }
    Ok(())
}

/// Extract common headers from an RFC 5322 message. Returns
/// `(message_id, in_reply_to, references_first, subject, date_epoch, from, to)`.
///
/// `references_first` is the FIRST Message-ID in the References
/// header — by RFC 5322 §3.6.4 that identifies the conversation root
/// (which is what monolith uses as the fastcore thread_id).
fn extract_headers(raw: &[u8]) -> (String, String, String, String, i64, String, String) {
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references_first = String::new();
    let mut subject = String::new();
    let mut date_epoch: i64 = 0;
    let mut from = String::new();
    let mut to = String::new();

    // We only need headers; stop at the first blank line.
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let head = &raw[..head_end];
    let s = String::from_utf8_lossy(head);
    // Unfold headers (RFC 5322 §2.2.3 — a header continues onto the
    // next line if that line starts with WSP).
    let mut cur = String::new();
    let mut lines: Vec<String> = Vec::new();
    for line in s.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            cur.push(' ');
            cur.push_str(line.trim_start());
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    for l in &lines {
        let Some((name, val)) = l.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match name.to_ascii_lowercase().as_str() {
            "message-id" => message_id = strip_angle(val),
            "in-reply-to" => in_reply_to = strip_angle(val),
            "references" => {
                // First token that looks like <...>
                references_first = val
                    .split_whitespace()
                    .find_map(|tok| {
                        let t = tok.trim_matches(|c: char| c == '<' || c == '>' || c == ',');
                        (!t.is_empty()).then(|| t.to_string())
                    })
                    .unwrap_or_default();
            }
            "subject" => subject = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "from" => from = val.to_string(),
            "to" => to = val.to_string(),
            "date" => date_epoch = parse_rfc5322_date(val).unwrap_or(0),
            _ => {}
        }
    }
    (
        message_id,
        in_reply_to,
        references_first,
        subject,
        date_epoch,
        from,
        to,
    )
}

fn strip_angle(v: &str) -> String {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        inner.trim().to_string()
    } else {
        t.trim_matches(|c: char| c == '<' || c == '>').to_string()
    }
}

/// Very small RFC 5322 date parser: `Wed, 01 Jul 2026 12:34:56 +0000`.
/// Only accepts `+0000`/`-0000`-style offsets; that covers everything
/// modern MTAs emit. Full parse coverage lives on `time` crate; we
/// don't need to pull it in for the fallback.
/// Parse an RFC 5322 `Date:` header value to unix epoch seconds (UTC).
///
/// Delegates to `chrono::DateTime::parse_from_rfc2822`, which handles
/// every real-world variant we see: `Sat, 13 Jun 2026 06:01:22 +0000`,
/// `Fri, 3 Jul 2026 02:40:42 +0900` (Gmail), `13 Jun 2026 06:01:22 GMT`
/// (no day-of-week), and named zones (`GMT`/`UTC`/`EST`/…). Timezones
/// are correctly normalised to UTC before the epoch conversion — the
/// previous hand-rolled parser dropped the zone entirely, so an email
/// stamped in JST landed nine hours off and inbound replies could sort
/// ahead of the sent copy.
///
/// Returns `None` when the header is empty / unparseable.
fn parse_rfc5322_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    // Retry ladder for the messy real world:
    //   1. Strip a trailing " (CFWS)" comment (RFC 5322 §3.3 permits it,
    //      chrono rejects it).
    //   2. Strip a leading "Weekday, " prefix — many senders ship a
    //      day-of-week that disagrees with the date (chrono treats that
    //      as Impossible even though the timestamp is well-formed).
    let no_comment = s.split(" (").next().unwrap_or(s).trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt.timestamp());
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow)
        .ok()
        .map(|dt| dt.timestamp())
}

/// Extract the delivery epoch from a Maildir filename. The Maildir
/// naming convention (`<epoch>.M<micro>P<pid>Q<seq>.<host>`) records
/// the delivery second in the leading component — a reliable fallback
/// when the message's `Date:` header is missing or unparseable. Filter
/// out obviously bogus epochs (<= year 2000) so we don't backdate
/// modern mail into 1970 territory.
fn maildir_filename_epoch(name: &str) -> Option<i64> {
    let first = name.split('.').next()?;
    let n: i64 = first.parse().ok()?;
    if n > 946_684_800 { Some(n) } else { None }
}

/// Fall back to the file's mtime as the delivery epoch when both the
/// `Date:` header and the maildir filename yield nothing usable.
fn file_mtime_epoch(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

#[derive(Debug, Clone)]
struct MailFile {
    /// blob_ref stored in the message wire. Either just the maildir
    /// filename (for INBOX/cur+new files) or `<subfolder>/<filename>`
    /// (for files under Maildir++ subfolders like `.Sent/`). The
    /// prefix lets `enrich_with_body` locate the file when it lives
    /// outside INBOX — otherwise `MaildirStore::fetch` returns None
    /// and the UI shows "(no text content)".
    filename: String,
    size: u32,
    message_id: String,
    in_reply_to: String,
    references_first: String,
    subject: String,
    date: i64,
    from: String,
    to: String,
}

/// Walk the user's maildir(s) and populate any thread whose messages
/// zset is empty. Best-effort — logs and continues on parse errors.
///
/// Coverage:
/// - INBOX (`cur/`, `new/`) and every top-level Maildir++ subfolder
///   under the user's maildir root (`.Sent/`, `.Drafts/`, `.Trash/`,
///   `.Junk/`, and any custom folder created by IMAP clients). Ensures
///   sent-copy messages get picked up even when they live in `.Sent`.
/// - Threading: for each file, resolve its "conversation root" via
///   References[0] → In-Reply-To → own Message-ID. All files sharing a
///   root get upserted into that root's fastcore thread.
async fn healed_from_maildir(state: &Arc<FastcoreState>, user: &str) {
    let Some((local, domain)) = user.split_once('@') else {
        return;
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(&root).join(domain).join(local);
    // Collect (subfolder_prefix, path) pairs. `subfolder_prefix` is
    // empty for INBOX and `.<foldername>` for Maildir++ subfolders.
    // It's later prepended to the blob_ref so `enrich_with_body` can
    // locate the file: INBOX files stay bare filenames (matches the
    // pg-dump migration's convention), subfolder files become
    // `.Sent/<filename>`.
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for sub in ["cur", "new"] {
        let dir = base.join(sub);
        if let Ok(iter) = std::fs::read_dir(&dir) {
            for entry in iter.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    files.push((String::new(), entry.path()));
                }
            }
        }
    }
    // Maildir++ subfolders (`.Sent`, `.Drafts`, `.Junk`, custom …).
    // IMAP clients that APPEND to `.Sent` write the user's outgoing
    // messages here — without walking these, the Sent tab is stuck at
    // "only threads whose sent-copy landed in INBOX via mirror_send".
    if let Ok(iter) = std::fs::read_dir(&base) {
        for entry in iter.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') {
                continue;
            }
            let sub_base = entry.path();
            for sub in ["cur", "new"] {
                let dir = sub_base.join(sub);
                if let Ok(iter) = std::fs::read_dir(&dir) {
                    for e in iter.flatten() {
                        if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                            files.push((name.clone(), e.path()));
                        }
                    }
                }
            }
        }
    }
    if files.is_empty() {
        return;
    }

    // Parse headers for every file. Only load the first 16 KB.
    let mut parsed: Vec<MailFile> = Vec::with_capacity(files.len());
    for (subfolder, path) in &files {
        let bare = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        // Prepend Maildir++ subfolder when set so `enrich_with_body`
        // can route `MaildirStore::fetch` to the right sub-maildir.
        let blob_ref = if subfolder.is_empty() {
            bare.clone()
        } else {
            format!("{subfolder}/{bare}")
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let size = bytes.len() as u32;
        let head = &bytes[..bytes.len().min(16 * 1024)];
        let (message_id, in_reply_to, references_first, subject, date, from, to) =
            extract_headers(head);
        if message_id.is_empty() {
            continue;
        }
        // If the RFC 5322 `Date:` header was missing or unparseable
        // (some mailers ship malformed dates and many self-injected
        // notifications have none), fall back to the maildir delivery
        // epoch encoded in the filename, then to file mtime, then to
        // 0. Without these fallbacks the affected messages sorted to
        // 1970 and inbound replies could end up ahead of the sent
        // copy in the thread timeline.
        let date = if date > 0 {
            date
        } else {
            maildir_filename_epoch(&bare)
                .or_else(|| file_mtime_epoch(path))
                .unwrap_or(0)
        };
        parsed.push(MailFile {
            filename: blob_ref,
            size,
            message_id,
            in_reply_to,
            references_first,
            subject,
            date,
            from,
            to,
        });
    }

    // Bucket by resolved conversation root.
    let mut by_root: std::collections::HashMap<String, Vec<&MailFile>> =
        std::collections::HashMap::new();
    for m in &parsed {
        let root = if !m.references_first.is_empty() {
            m.references_first.clone()
        } else if !m.in_reply_to.is_empty() {
            m.in_reply_to.clone()
        } else {
            m.message_id.clone()
        };
        by_root.entry(root).or_default().push(m);
    }

    // UID backfill — one-time per boot per user. Repair any
    // MessageWire that self-heal wrote before we started allocating
    // uids (all showed uid=0, breaking /api/mail/messages/{uid}/…
    // attachment endpoints). Guard on a persistent flag so subsequent
    // ticks don't re-scan the full mailbox. Bump the sentinel key when
    // the migration format changes to force another sweep.
    let uid_flag_key = format!("mailrs:user:{user}:uid_backfill_v1");
    let need_uid_backfill = state
        .mailbox
        .store_ref()
        .get(uid_flag_key.as_bytes())
        .ok()
        .flatten()
        .is_none();
    if need_uid_backfill {
        let by_activity = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
        let all_tids = state
            .mailbox
            .store_ref()
            .zrevrange(by_activity.as_bytes(), 0, -1)
            .unwrap_or_default();
        let mut uid_healed = 0u32;
        for (tid_bytes, _score) in all_tids {
            let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
                continue;
            };
            let msgs = state.mailbox.list_thread_messages(tid).unwrap_or_default();
            for payload in msgs {
                let Ok(mut wire) = serde_json::from_slice::<
                    mailrs_core_api::method::message::MessageWire,
                >(&payload) else {
                    continue;
                };
                if wire.uid != 0 {
                    continue;
                }
                let uid = state
                    .mailbox
                    .allocate_uid(user, &wire.message_id)
                    .unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                wire.uid = uid;
                if let Ok(new_payload) = serde_json::to_vec(&wire) {
                    let _ = state.mailbox.upsert_message(
                        &wire.thread_id,
                        &wire.message_id,
                        wire.internal_date,
                        &new_payload,
                    );
                }
                uid_healed += 1;
            }
        }
        let _ = state.mailbox.store_ref().set(uid_flag_key.as_bytes(), b"1");
        if uid_healed > 0 {
            tracing::info!(%user, uid_healed, "self-heal: uid backfill (one-shot)");
        }
    }

    // Walk empty-messages threads and heal each from its bucket.
    let activity_key = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
    let tids = state
        .mailbox
        .store_ref()
        .zrevrange(activity_key.as_bytes(), 0, 999)
        .unwrap_or_default();
    let mut healed_threads = 0u32;
    let mut healed_msgs = 0u32;
    for (tid_bytes, _score) in tids {
        let Ok(tid) = std::str::from_utf8(&tid_bytes) else {
            continue;
        };
        let msg_zset = mailrs_mailbox_kevy::keys::thread_messages(tid);
        if state
            .mailbox
            .store_ref()
            .zcard(msg_zset.as_bytes())
            .unwrap_or(0)
            > 0
        {
            continue;
        }
        let Some(bucket) = by_root.get(tid) else {
            continue;
        };
        // Sort by date so upsert_message's zadd scores are chronological.
        let mut ordered: Vec<&MailFile> = bucket.to_vec();
        ordered.sort_by_key(|m| m.date);
        for m in &ordered {
            // Allocate a stable per-user uid before writing the wire so
            // /api/mail/messages/{uid}/attachments/… can resolve the
            // message. allocate_uid is idempotent — reruns return the
            // previously-issued uid via the uid_by_mid reverse index.
            let uid = state.mailbox.allocate_uid(user, &m.message_id).unwrap_or(0);
            let wire = mailrs_core_api::method::message::MessageWire {
                id: 0,
                mailbox_id: 0,
                uid,
                blob_ref: m.filename.clone(),
                sender: m.from.clone(),
                recipients: m.to.clone(),
                subject: m.subject.clone(),
                date: m.date,
                internal_date: m.date,
                size: m.size,
                flags: 1,
                message_id: m.message_id.clone(),
                in_reply_to: m.in_reply_to.clone(),
                thread_id: tid.to_string(),
                modseq: 0,
                user_address: user.to_string(),
            };
            let payload = match serde_json::to_vec(&wire) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let _ = state
                .mailbox
                .upsert_message(tid, &wire.message_id, m.date, &payload);
            healed_msgs += 1;
        }
        healed_threads += 1;
    }
    if healed_threads > 0 {
        tracing::info!(
            %user, healed_threads, healed_msgs, files_scanned = parsed.len(),
            "self-heal (maildir): populated missing messages"
        );
    }

    // ── Sent-index backfill ──────────────────────────────────────
    //
    // The historical migration derived senders_csv from monolith's
    // sent_count aggregate, which false-negatives on many threads the
    // user actually sent messages to (only 22 of ~200 sent messages
    // were picked up on lihao@golia.jp). Walk every thread bucket and,
    // if any of its maildir files has From: == user, add the thread's
    // fastcore tid to `mailrs:user:<u>:threads:sent` scored by the
    // latest sent message's date. Idempotent: zadd overwrites the
    // score for a tid that's already there.
    let sent_key = mailrs_mailbox_kevy::keys::user_threads_sent(user);
    let mut sent_added = 0u32;
    let mut created = 0u32;
    for (root, bucket) in &by_root {
        let sent_here: Vec<&&MailFile> = bucket
            .iter()
            .filter(|m| mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .collect();
        let is_sender_thread = !sent_here.is_empty();
        let thread_key = mailrs_mailbox_kevy::keys::thread(root);
        let exists = state
            .mailbox
            .store_ref()
            .hexists(thread_key.as_bytes(), b"count")
            .unwrap_or(false);
        if !exists {
            // Create a minimal thread aggregate from scratch — inbound
            // OR outbound. Skipping non-sender threads here was the
            // reason fresh Gmail arrivals (files present in maildir but
            // no matching kevy hash) never showed up in the inbox: the
            // "heal missing messages" branch above only touches threads
            // already in the by_activity zset, so a genuinely new
            // arrival had no path in. Create here for every bucket.
            let mut ordered: Vec<&MailFile> = bucket.to_vec();
            ordered.sort_by_key(|m| m.date);
            for m in &ordered {
                let category = "inbox";
                let unread = !mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user);
                let arrival = mailrs_mailbox_kevy::MessageArrival {
                    thread_id: root,
                    user,
                    subject: &m.subject,
                    senders_csv: &m.from,
                    latest_date: m.date,
                    latest_preview: "",
                    category,
                    unread,
                };
                let _ = state.mailbox.record_message_arrival(&arrival);
                // Side sinks: contacts autocomplete + Meili index.
                crate::live_sync::upsert_contacts(user, &m.from);
                crate::live_sync::index_meili(user, root, &m.subject, &m.from, "", m.date);
                // Also write the message blob for enrich_with_body.
                let uid = state.mailbox.allocate_uid(user, &m.message_id).unwrap_or(0);
                let wire = mailrs_core_api::method::message::MessageWire {
                    id: 0,
                    mailbox_id: 0,
                    uid,
                    blob_ref: m.filename.clone(),
                    sender: m.from.clone(),
                    recipients: m.to.clone(),
                    subject: m.subject.clone(),
                    date: m.date,
                    internal_date: m.date,
                    size: m.size,
                    flags: 1,
                    message_id: m.message_id.clone(),
                    in_reply_to: m.in_reply_to.clone(),
                    thread_id: root.clone(),
                    modseq: 0,
                    user_address: user.to_string(),
                };
                if let Ok(payload) = serde_json::to_vec(&wire) {
                    let _ = state
                        .mailbox
                        .upsert_message(root, &m.message_id, m.date, &payload);
                }
            }
            created += 1;
        }
        if !is_sender_thread {
            // Inbound-only thread — created (or already existed), but
            // the user isn't a sender so it doesn't belong in the sent
            // zset. Skip the sent-index maintenance below.
            continue;
        }
        // Score the sent zset by the aggregate's own latest_date —
        // that's what the UI displays as the row's date pill, so this
        // guarantees the list is sorted identically to what the pill
        // shows. Prior versions scored by our own local bucket-max,
        // which drifted from the aggregate whenever record_message_arrival
        // was called separately (e.g. via mirror_send during a live
        // send) and left the two in disagreement → apparent random order.
        //
        // If the stored aggregate latest_date is stale/zero (e.g. the
        // hash was created back when parse_rfc5322_date was broken and
        // fed 0), prefer the bucket's true max date and heal the hash
        // + by_activity index so the row stops sinking to the bottom.
        let stored_latest = state
            .mailbox
            .store_ref()
            .hget(thread_key.as_bytes(), b"latest_date")
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let bucket_max = bucket.iter().map(|m| m.date).max().unwrap_or(0);
        let agg_latest = std::cmp::max(stored_latest, bucket_max);
        if agg_latest > stored_latest {
            let _ = state.mailbox.store_ref().hset(
                thread_key.as_bytes(),
                &[(b"latest_date" as &[u8], agg_latest.to_string().as_bytes())],
            );
            let by_activity = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
            let _ = state.mailbox.store_ref().zadd(
                by_activity.as_bytes(),
                &[(agg_latest as f64, root.as_bytes())],
            );
        }
        let _ = state
            .mailbox
            .store_ref()
            .zadd(sent_key.as_bytes(), &[(agg_latest as f64, root.as_bytes())]);
        // Also merge user into the thread's senders_csv so future
        // upsert_thread invocations (mark_read etc.) don't drop the
        // sent-index membership.
        let cur_csv = state
            .mailbox
            .store_ref()
            .hget(thread_key.as_bytes(), b"senders_csv")
            .unwrap_or_default()
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_default();
        if !mailrs_mailbox_kevy::senders_csv_contains_user(&cur_csv, user) {
            let new_csv = if cur_csv.is_empty() {
                user.to_string()
            } else {
                format!("{cur_csv}, {user}")
            };
            let _ = state.mailbox.store_ref().hset(
                thread_key.as_bytes(),
                &[(b"senders_csv" as &[u8], new_csv.as_bytes())],
            );
        }
        sent_added += 1;
    }
    if sent_added > 0 || created > 0 {
        tracing::info!(
            %user, sent_added, created,
            "self-heal (maildir): sent-index backfill"
        );
    }
}

fn build_router(state: Arc<FastcoreState>) -> Router {
    let base = base_router(state.clone());
    // One Router for all business routes so matchit's trie sees the
    // full set at once. Earlier split into convo + thread Routers
    // hit a route-resolution bug where only the first-registered
    // route under /v1/users/{user}/conversations matched at runtime —
    // probable matchit collision between `conversations:list` (literal
    // ":list") and `conversations/categories` (path-separator). A
    // single Router with all routes registered side-by-side resolves it.
    let business = Router::new()
        .route(conv::PATH_LIST_CONVERSATIONS, post(list_conversations))
        .route(conv::PATH_CONVERSATION_CATEGORIES, get(get_categories))
        .route(conv::PATH_ACTION_COUNT, get(get_action_count))
        .route(conv::PATH_UNSEEN_COUNT, get(get_unseen_count))
        .route(th::PATH_LIST_THREAD_MESSAGES, get(thread_messages))
        .route(th::PATH_DELIVER_MESSAGE, post(deliver_message))
        .route(th::PATH_MARK_READ, post(mark_read))
        .route(th::PATH_MARK_ALL_READ, post(mark_all_read_route))
        .route(th::PATH_MARK_UNREAD, post(mark_unread_route))
        .route(th::PATH_SNOOZE, put(snooze_thread_route))
        .route(th::PATH_UNSNOOZE, delete(unsnooze_thread_route))
        .route(th::PATH_PIN, post(pin_thread))
        .route(th::PATH_UNPIN, post(unpin_thread))
        .route(th::PATH_STAR, post(star_thread))
        .route(th::PATH_UNSTAR, post(unstar_thread))
        .route(th::PATH_ARCHIVE, post(archive_thread))
        .route(th::PATH_UNARCHIVE, post(unarchive_thread))
        .route(th::PATH_DISMISS_ACTION, post(dismiss_action))
        .route(th::PATH_DELETE_THREAD, delete(delete_thread))
        .route(adm::PATH_GET_ACCOUNT_HASH, get(get_account_with_hash))
        .route(adm::PATH_EFFECTIVE_PERMISSIONS, get(effective_permissions))
        .route(
            adm::PATH_LIST_ACCOUNTS,
            get(list_accounts).post(add_account_route),
        )
        .route(
            adm::PATH_UPDATE_ACCOUNT,
            put(update_account_route).delete(remove_account_route),
        )
        .route(adm::PATH_SET_QUOTA, post(set_quota_route))
        .route(
            adm::PATH_UPDATE_RECOVERY_EMAIL,
            post(set_recovery_email_route),
        )
        .route(adm::PATH_SET_ACCOUNT_PASSWORD, post(set_password_route))
        .route(adm::PATH_SET_MESSAGE_FLAGS, post(set_message_flags_route))
        // Aliases live in the fastcore-embedded kevy so the spool drain
        // (also in-process) can resolve `contact@golia.jp -> lihao` and
        // similar single-hop forwards. Distinct namespace from webapi's
        // network-kevy `admin:aliases` hash — that older store is not
        // consulted by the drain and stays around only until UI wiring
        // catches up.
        .route(
            "/v1/admin/aliases:local",
            get(list_local_aliases).post(upsert_local_alias),
        )
        .route(
            "/v1/admin/aliases:local/{source}",
            delete(delete_local_alias_route),
        )
        // Ops endpoint — reset every user's ingest cursor to 0 so the
        // next sync tick re-processes historic threads and (via the
        // Group F diff path) backfills messages fastcore missed under
        // the older "skip-existing" ingest behaviour.
        .route(
            "/v1/admin/sync/reset-cursors",
            post(reset_sync_cursors_route),
        )
        .route(mb::PATH_LIST_MAILBOXES, get(list_mailboxes))
        .route(
            msg::PATH_GET_MESSAGE_BY_UID_USER,
            get(get_message_by_uid_for_user),
        )
        .with_state(state);

    base.merge(business)
}

fn row_to_wire(r: ThreadRow) -> ConversationSummaryWire {
    ConversationSummaryWire {
        thread_id: r.thread_id,
        subject: r.subject,
        participants: r.senders_csv,
        message_count: r.count.max(0) as u32,
        unread_count: r.unread_count.max(0) as u32,
        last_date: r.latest_date,
        category: r.category,
        flagged: r.requires_action,
        snippet: r.latest_preview,
        pinned: r.pinned,
        archived: r.archived,
        importance_level: r.importance_level,
        importance_score: r.importance_score as f32,
        requires_action: r.requires_action,
        last_sender: String::new(), // not yet tracked on the kevy row
        sent_count: r.sent_count.max(0) as u32,
    }
}

/// `POST /v1/users/{user}/conversations:list`.
async fn list_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<conv::ListConversationsResponse> {
    let f = &req.filter;
    let filter = ListThreadsFilter {
        category: f.category.as_deref(),
        folder: f.folder.as_deref(),
        pinned: false,
        archived: f.archived,
        has_unread: f.unread.unwrap_or(false),
        has_action: false,
        starred: f.starred.unwrap_or(false),
        before_ts: f.before_ts,
    };
    let limit = if f.limit == 0 { 50 } else { f.limit as usize };
    let (rows, _total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|_| (Vec::new(), 0));

    let items = rows.into_iter().map(row_to_wire).collect();
    Json(conv::ListConversationsResponse { items })
}

/// `GET /v1/users/{user}/conversations/categories` — histogram of
/// category → distinct thread_id count, read straight off the per-
/// category zsets.
async fn get_categories(
    State(state): State<Arc<FastcoreState>>,
    Path(_user): Path<String>,
) -> Json<conv::ConversationCategoriesResponse> {
    // Expanded set — covers monolith's known category vocabulary.
    // Any per-category zset that ZCARD > 0 is returned. UI tabs only
    // render the categories that exist so overshooting is safe.
    let candidates = [
        "inbox",
        "personal",
        "bulk",
        "spam",
        "scam",
        "promotions",
        "updates",
        "forums",
        "work",
        "notifications",
        "receipts",
        "newsletter",
    ];
    let categories: Vec<conv::CategoryCount> = candidates
        .into_iter()
        .map(|cat| {
            let key = mailrs_mailbox_kevy::keys::user_threads_by_category(&_user, cat);
            let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
            conv::CategoryCount {
                category: cat.to_string(),
                count,
            }
        })
        .filter(|c| c.count > 0)
        .collect();
    Json(conv::ConversationCategoriesResponse { categories })
}

/// `GET /v1/users/{user}/conversations/action-count` — single ZCARD on
/// the has_action zset.
async fn get_action_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::ActionCountResponse> {
    let key = mailrs_mailbox_kevy::keys::user_threads_has_action(&user);
    let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
    Json(conv::ActionCountResponse { count })
}

/// `GET /v1/users/{user}/conversations/unseen-count` — single ZCARD on
/// the has_unread zset.
async fn get_unseen_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::UnseenCountResponse> {
    let key = mailrs_mailbox_kevy::keys::user_threads_has_unread(&user);
    let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
    Json(conv::UnseenCountResponse { count })
}

/// `GET /v1/users/{user}/threads/{thread_id}/messages` — returns the
/// thread's messages. mailbox-kevy doesn't store per-message rows yet
/// (only the aggregate row), so this returns the empty list until
/// Phase 7.11 lands a per-message layout. Webapi treats empty as
/// "thread exists but currently rendering, retry shortly" — graceful
/// fallback that keeps the UI from 404-ing the whole conversation
/// view while the kevy data shape grows.
async fn thread_messages(
    State(state): State<Arc<FastcoreState>>,
    Path((_user, thread_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::ListThreadMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    let blobs = state
        .mailbox
        .list_thread_messages(&thread_id)
        .unwrap_or_default();
    let items = blobs
        .into_iter()
        .filter_map(|b| serde_json::from_slice::<MessageWire>(&b).ok())
        .collect();
    Json(mailrs_core_api::method::thread::ListThreadMessagesResponse { items })
}

// ── Account (auth) — Phase 8 ────────────────────────────────────────

/// `GET /v1/admin/accounts/{address}/credentials` — used by webapi's
/// login handler to fetch the argon2 hash. Blob in kevy is a JSON
/// AccountWithHashWire; we forward it verbatim.
async fn get_account_with_hash(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_account_blob(&address) {
        Ok(Some(json)) => Ok(([("content-type", "application/json")], json).into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_account_blob failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/admin/accounts/{address}/effective-permissions`.
async fn effective_permissions(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_permissions_blob(&address) {
        Ok(Some(json)) => Ok(([("content-type", "application/json")], json).into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_permissions_blob failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/admin/accounts` — walk kevy account index + return
/// AccountListResponse. Zero spg.
async fn list_accounts(State(state): State<Arc<FastcoreState>>) -> Json<adm::AccountListResponse> {
    let mut items = Vec::new();
    let addrs = state.mailbox.list_account_addresses().unwrap_or_default();
    for addr in addrs {
        if let Ok(Some(json)) = state.mailbox.get_account_blob(&addr)
            && let Ok(acc) = serde_json::from_str::<adm::AccountWithHashWire>(&json)
        {
            items.push(acc.public);
        }
    }
    Json(adm::AccountListResponse { items })
}

/// `GET /v1/users/{user}/messages/by-uid/{uid}` — look up a message by
/// the user-scoped uid index (populated by `deliver_message` /
/// `mailrs-fastcore-backfill-uid-index`). Returns the JSON MessageWire.
async fn get_message_by_uid_for_user(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(bytes)) => Ok((
            [("content-type", "application/json")],
            String::from_utf8(bytes).unwrap_or_default(),
        )
            .into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %user, %uid, "get_message_by_uid failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ── Mailboxes (folders) ────────────────────────────────────────────

/// `GET /v1/users/{user}/mailboxes` — returns the INBOX + standard IMAP
/// folders. Counts derived from kevy zsets so no spg touch.
/// This is a minimum-viable shape — future phase populates true
/// per-mailbox metadata via mailbox-kevy `list_mailboxes` when the
/// mailbox → messages sub-index lands.
async fn list_mailboxes(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<mailrs_core_api::method::mailbox::ListMailboxesResponse> {
    use mailrs_core_api::method::mailbox::{ListMailboxesResponse, MailboxWire};
    let total = state
        .mailbox
        .store_ref()
        .zcard(mailrs_mailbox_kevy::keys::user_threads_by_activity(&user).as_bytes())
        .unwrap_or(0) as u32;
    let unseen = state
        .mailbox
        .store_ref()
        .zcard(mailrs_mailbox_kevy::keys::user_threads_has_unread(&user).as_bytes())
        .unwrap_or(0) as u32;
    let items = vec![
        MailboxWire {
            id: 1,
            user: user.clone(),
            name: "INBOX".to_string(),
            uidvalidity: 1,
            uidnext: total + 1,
            highest_modseq: total as u64,
        },
        MailboxWire {
            id: 2,
            user: user.clone(),
            name: "Sent".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 3,
            user: user.clone(),
            name: "Drafts".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 4,
            user: user.clone(),
            name: "Junk".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 5,
            user,
            name: "Trash".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
    ];
    let _ = unseen;
    Json(ListMailboxesResponse { items })
}

// ── Thread mutations ───────────────────────────────────────────────

/// Uniform mutation response — matches monolith's `ThreadActionResponse`
/// JSON shape so the core-rpc client's deserializer succeeds. Fastcore's
/// mutations are idempotent (mark_seen / set_pinned / set_starred / ...
/// are all noop-safe when the target thread is already in the requested
/// state or missing). Return 200 unconditionally so the UI's optimistic
/// patch never rolls back — a missing thread row simply means "nothing
/// to do" and the list refetch will reconcile.
fn action_result(_found: bool) -> axum::response::Response {
    use axum::response::IntoResponse;
    Json(th::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
    .into_response()
}

async fn mark_read(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.mark_seen(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_seen io error — treating as noop");
    }
    action_result(true)
}

/// POST `/v1/users/{user}/conversations:mark-all-read` — sweep every
/// unread thread and flip it to seen in one call. UI's "Mark all as
/// read" button was previously batching only the loaded pagination
/// slice, so users with 99+ unread across pages left the tail
/// untouched.
async fn mark_all_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<serde_json::Value> {
    let flipped = state.mailbox.mark_all_seen(&user).unwrap_or(0);
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

async fn pin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn star_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_starred(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn unstar_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_starred(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn unpin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn archive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_archived(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn unarchive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_archived(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn dismiss_action(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_has_action(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn delete_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .delete_thread(&user, &thread_id)
            .unwrap_or(false),
    )
}

use axum::response::IntoResponse;

async fn mark_unread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.mark_unread(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_unread io error — treating as noop");
    }
    action_result(true)
}

async fn snooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::SnoozeRequest>,
) -> axum::response::Response {
    if let Err(e) = state
        .mailbox
        .set_snoozed(&user, &thread_id, req.snoozed_until)
    {
        tracing::warn!(error = %e, %user, %thread_id, "snooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn unsnooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.set_snoozed(&user, &thread_id, 0) {
        tracing::warn!(error = %e, %user, %thread_id, "unsnooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// POST /v1/users/{user}/threads/{thread_id}/messages — the sent /
/// draft / import write path. Mirrors what the inbound ingest loop
/// does, but the caller controls the metadata (senders_csv, unread,
/// category) so it can synthesize a "user is the sender" arrival.
///
/// Executes 3 atomic-ish steps:
///   1. `record_message_arrival` — thread aggregate + activity/category
///      zsets + has_unread toggle if `unread=true`
///   2. `upsert_message` — write `mailrs:msg:<mid>` blob (verbatim
///      `payload_wire_json`) + zadd `mailrs:thread:<tid>:messages`
///   3. `upsert_thread` — re-read the aggregate we just updated and
///      re-emit every index, most importantly `user_threads_sent` (adds
///      when `senders_csv_contains_user`) and `has_unread`
async fn deliver_message(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::DeliverMessageRequest>,
) -> axum::response::Response {
    use mailrs_mailbox_kevy::MessageArrival;
    let arrival = MessageArrival {
        thread_id: &thread_id,
        user: &user,
        subject: &req.subject,
        senders_csv: &req.senders_csv,
        latest_date: req.latest_date,
        latest_preview: &req.latest_preview,
        category: &req.category,
        unread: req.unread,
    };

    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::error!(err = %e, %user, %thread_id, "record_message_arrival failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Side sinks so contacts autocomplete + Meili stay live on webapi-
    // driven deliveries (mirror-send, forward-into-thread, etc.).
    crate::live_sync::upsert_contacts(&user, &req.senders_csv);
    crate::live_sync::index_meili(
        &user,
        &thread_id,
        &req.subject,
        &req.senders_csv,
        &req.latest_preview,
        req.latest_date,
    );

    if let Err(e) = state.mailbox.upsert_message(
        &thread_id,
        &req.message_id,
        req.latest_date,
        req.payload_wire_json.as_bytes(),
    ) {
        tracing::error!(err = %e, %user, %thread_id, "upsert_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Re-emit thread row so index zsets (sent, has_unread, etc.) reflect
    // the new senders_csv / unread_count state. We read the row we just
    // wrote and hand it to upsert_thread which owns the index fanout.
    match state.mailbox.get_thread(&thread_id) {
        Ok(Some(row)) => {
            if let Err(e) = state.mailbox.upsert_thread(&user, &row) {
                tracing::warn!(err = %e, %user, %thread_id, "upsert_thread reindex failed");
            }
        }
        Ok(None) => {
            tracing::warn!(%user, %thread_id, "get_thread returned None right after write");
        }
        Err(e) => {
            tracing::warn!(err = %e, %user, %thread_id, "get_thread failed");
        }
    }

    if req.uid > 0
        && let Err(e) = state.mailbox.index_uid(&user, req.uid, &req.message_id)
    {
        tracing::warn!(err = %e, %user, uid = req.uid, "index_uid failed");
    }

    Json(th::DeliverMessageResponse {
        thread_id,
        message_id: req.message_id,
    })
    .into_response()
}

// ── Group B: admin write handlers ─────────────────────────────────
//
// The webapi used to write account / permission / message blobs to
// the network kevy directly (`MAILRS_KEVY_URL`). Fastcore reads its
// own embedded kevy at `/data/kevy-fastcore`, so those writes never
// affected login / account list / update_flags. These handlers close
// the gap: webapi calls fastcore RPCs, fastcore mutates its embedded
// kevy through the same `KevyMailboxStore` used at boot / ingest.

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn add_account_route(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<adm::AddAccountRequest>,
) -> axum::response::Response {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    let salt = SaltString::generate(&mut ArgonRng);
    let hash = match Argon2::default().hash_password(req.password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let domain = req
        .address
        .split_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let blob = serde_json::json!({
        "address": &req.address,
        "domain": domain,
        "display_name": req.display_name,
        "active": true,
        "created_at": now_secs(),
        "quota_bytes": 10_737_418_240i64,
        "recovery_email": null,
        "password_hash": hash,
    });
    let json = blob.to_string();
    if let Err(e) = state.mailbox.upsert_account(&req.address, &json) {
        tracing::error!(err = %e, addr = %req.address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let perms = serde_json::json!({
        "address": &req.address,
        "permissions": Vec::<String>::new(),
        "groups": Vec::<serde_json::Value>::new(),
        "is_super": false,
        "send_as": Vec::<String>::new(),
    })
    .to_string();
    if let Err(e) = state.mailbox.upsert_permissions(&req.address, &perms) {
        tracing::warn!(err = %e, addr = %req.address, "upsert_permissions failed");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn update_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateAccountRequest>,
) -> axum::response::Response {
    let cur = match state.mailbox.get_account_blob(&address) {
        Ok(Some(s)) => s,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut val: serde_json::Value = match serde_json::from_str(&cur) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert(
            "display_name".into(),
            serde_json::Value::String(req.display_name),
        );
    }
    let json = val.to_string();
    if let Err(e) = state.mailbox.upsert_account(&address, &json) {
        tracing::error!(err = %e, %address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn remove_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.delete_account(&address) {
        tracing::warn!(err = %e, %address, "delete_account failed");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn set_quota_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetQuotaRequest>,
) -> axum::response::Response {
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "quota_bytes".into(),
            serde_json::Value::from(req.quota_bytes),
        );
    })
    .await
}

async fn set_recovery_email_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateRecoveryEmailRequest>,
) -> axum::response::Response {
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "recovery_email".into(),
            serde_json::Value::String(req.recovery_email),
        );
    })
    .await
}

async fn set_password_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetPasswordRequest>,
) -> axum::response::Response {
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "password_hash".into(),
            serde_json::Value::String(req.password_hash),
        );
    })
    .await
}

async fn patch_account_field<F>(
    state: &Arc<FastcoreState>,
    address: &str,
    mutator: F,
) -> axum::response::Response
where
    F: FnOnce(&mut serde_json::Map<String, serde_json::Value>),
{
    let cur = match state.mailbox.get_account_blob(address) {
        Ok(Some(s)) => s,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut val: serde_json::Value = match serde_json::from_str(&cur) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(obj) = val.as_object_mut() {
        mutator(obj);
    }
    let json = val.to_string();
    if let Err(e) = state.mailbox.upsert_account(address, &json) {
        tracing::error!(err = %e, %address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// `POST /v1/admin/sync/reset-cursors` — reset every registered
/// user's `mailrs:sync:cursor:<user>` key so the next
/// `ingest_sync_loop` tick treats every monolith thread as "new" and
/// runs the Group F diff path to backfill missing messages.
async fn reset_sync_cursors_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let addrs = match state.mailbox.list_account_addresses() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut cleared = 0u32;
    for user in &addrs {
        let key = format!("mailrs:sync:cursor:{user}");
        if state.mailbox.store_ref().del(&[key.as_bytes()]).is_ok() {
            cleared += 1;
        }
    }
    Json(serde_json::json!({ "cleared": cleared })).into_response()
}

/// `POST /v1/users/{user}/messages/{uid}/flags` — patch the flags
/// bitmask on a message blob. Also reconciles the thread's `has_unread`
/// zset via `mark_seen` / `mark_unread` when `\Seen` toggled.
async fn set_message_flags_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
    Json(req): Json<adm::SetMessageFlagsRequest>,
) -> axum::response::Response {
    let bytes = match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(b)) => b,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut wire: mailrs_core_api::method::message::MessageWire =
        match serde_json::from_slice(&bytes) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(err = %e, %user, %uid, "wire parse failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    let old_flags = wire.flags;
    let new_flags = req.flags;
    wire.flags = new_flags;
    let json = match serde_json::to_vec(&wire) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(e) =
        state
            .mailbox
            .upsert_message(&wire.thread_id, &wire.message_id, wire.date, &json)
    {
        tracing::error!(err = %e, %user, %uid, "upsert_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let seen_bit = 0b0000_0001u32;
    let was_seen = (old_flags & seen_bit) != 0;
    let is_seen = (new_flags & seen_bit) != 0;
    if was_seen != is_seen && !wire.thread_id.is_empty() {
        let _ = if is_seen {
            state.mailbox.mark_seen(&user, &wire.thread_id)
        } else {
            state.mailbox.mark_unread(&user, &wire.thread_id)
        };
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// GET `/v1/admin/aliases:local` — list every fastcore-embedded alias.
async fn list_local_aliases(State(state): State<Arc<FastcoreState>>) -> Json<serde_json::Value> {
    let items = state.mailbox.list_aliases().unwrap_or_default();
    let payload: Vec<serde_json::Value> = items
        .into_iter()
        .map(|(source, target)| serde_json::json!({"source": source, "target": target}))
        .collect();
    Json(serde_json::json!({ "items": payload }))
}

#[derive(serde::Deserialize)]
struct AliasBody {
    source: String,
    target: String,
}

/// POST `/v1/admin/aliases:local` — insert/replace one alias entry.
async fn upsert_local_alias(
    State(state): State<Arc<FastcoreState>>,
    Json(body): Json<AliasBody>,
) -> axum::response::Response {
    if body.source.is_empty() || body.target.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    match state.mailbox.upsert_alias(&body.source, &body.target) {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, source = %body.source, "upsert_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE `/v1/admin/aliases:local/{source}` — drop one alias entry.
async fn delete_local_alias_route(
    State(state): State<Arc<FastcoreState>>,
    Path(source): Path<String>,
) -> axum::response::Response {
    match state.mailbox.delete_alias(&source) {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, %source, "delete_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use mailrs_mailbox_kevy::MessageArrival;
    use tower::ServiceExt;

    fn fresh_state() -> Arc<FastcoreState> {
        let store = Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("in-memory kevy"),
        );
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState { mailbox })
    }

    fn arr<'a>(tid: &'a str, user: &'a str, unread: bool) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread,
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn healthz_reports_kevy_backend() {
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        assert!(body.contains("\"backend\":\"kevy\""), "{body}");
    }

    #[tokio::test]
    async fn unseen_count_after_arrival_is_one() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/users/u@x.com/conversations/unseen-count")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(body_string(resp).await.contains("\"count\":1"));
    }

    #[tokio::test]
    async fn mark_read_drops_from_unseen() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/t1/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            state
                .mailbox
                .get_thread("t1")
                .unwrap()
                .unwrap()
                .unread_count,
            0
        );
    }

    #[tokio::test]
    async fn mark_read_on_missing_returns_200_idempotent() {
        // Post 5eb8cc07 mutations are idempotent — a missing thread row
        // returns 200 (noop success) instead of 404 so the UI's optimistic
        // patch doesn't flicker back to unread. Reconciliation happens on
        // the next list refetch.
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/nope/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn list_conversations_returns_arrivals() {
        let state = fresh_state();
        for i in 0..3 {
            state
                .mailbox
                .record_message_arrival(&MessageArrival {
                    thread_id: &format!("t{i}"),
                    user: "u@x.com",
                    subject: "Subj",
                    senders_csv: "x@y.z",
                    latest_date: i as i64 * 100,
                    latest_preview: "preview",
                    category: "inbox",
                    unread: true,
                })
                .unwrap();
        }
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/conversations:list")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"limit":10}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        // reverse chronological → t2 first
        assert!(body.contains(r#""thread_id":"t2""#));
    }

    /// Smoke every business route — verifies no 404 from a router-
    /// resolution bug. Each route is hit with a request that should
    /// land on the handler; expected statuses are documented inline
    /// (the handler's own 204/404 logic is what we then assert).
    #[tokio::test]
    async fn every_route_resolves_no_404() {
        let state = fresh_state();
        // Seed one thread + one message so the routes have a real
        // target to flip / read.
        state
            .mailbox
            .deliver_message(&arr("t1", "u@x.com", true), "m1", b"{}")
            .unwrap();

        struct Probe {
            method: Method,
            uri: &'static str,
            allowed: &'static [u16],
        }
        let probes: &[Probe] = &[
            // Conversations
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/conversations:list",
                allowed: &[200, 415, 422],
            }, // 415/422 if empty body, 200 with body
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/categories",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/action-count",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/unseen-count",
                allowed: &[200],
            },
            // Thread read
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/threads/t1/messages",
                allowed: &[200],
            },
            // Thread mutations (return 204 on existing tid, 404 on missing)
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/read",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/pin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unpin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/star",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unstar",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/archive",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unarchive",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/dismiss-action",
                allowed: &[200],
            },
            Probe {
                method: Method::DELETE,
                uri: "/v1/users/u@x.com/threads/t1",
                allowed: &[200],
            }, // delete after archive may already be gone
            // Probes
            Probe {
                method: Method::GET,
                uri: "/v1/healthz",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/readyz",
                allowed: &[200],
            },
        ];

        for p in probes {
            let app = build_router(state.clone());
            let body = if p.method == Method::POST && p.uri.ends_with(":list") {
                Body::from(r#"{"limit":10}"#)
            } else {
                Body::empty()
            };
            let resp = app
                .oneshot(
                    Request::builder()
                        .method(p.method.clone())
                        .uri(p.uri)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            let code = resp.status().as_u16();
            assert!(
                p.allowed.contains(&code),
                "{} {} returned {code}, expected {:?}",
                p.method,
                p.uri,
                p.allowed
            );
            assert_ne!(code, 404, "router did not match: {} {}", p.method, p.uri);
        }
    }
}
