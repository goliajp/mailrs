//! Bayesian spam-corpus training + storage, network-kevy backed.
//!
//! The pure math lives in the `mailrs-bayes` stone; this module is the
//! cement/steel glue: read a thread's raw messages from the maildir,
//! tokenize them, and HINCRBY the per-token spam/ham counts into the
//! shared network kevy so the receiver process (a different binary)
//! can classify at SMTP time.
//!
//! Storage keys (network kevy): see `mailrs_triage::spam`. A
//! `bayes:v2:trained:<thread_id>` marker records the last training
//! direction so re-marking a thread (junk → not-junk) unlearns before it
//! re-learns.

use std::sync::Arc;

use crate::FastcoreState;

/// "Every row", for the declared queries that take a limit.
///
/// Named rather than a bare number so a reader can see it is not a page
/// size: these are one-shot maintenance passes over a whole mailbox, and a
/// silent cap here would seed the corpus from an arbitrary slice while
/// reporting the count it happened to reach.
const ALL: usize = usize::MAX;

/// Train the classifier on one thread. `is_spam=true` learns it as
/// spam, `false` as ham. Idempotent + reversible: if the thread was
/// previously trained the other way, that direction is unlearned first
/// (counts decremented) so a mis-file correction doesn't double-count.
///
/// Best-effort — a kevy hiccup or a missing maildir file is logged and
/// swallowed; training must never fail the user-facing mark action.
pub fn train_thread(state: &Arc<FastcoreState>, user: &str, thread_id: &str, is_spam: bool) {
    let raws = fetch_thread_raw(state, user, thread_id);
    if raws.is_empty() {
        return;
    }
    let Some(mut conn) = state.net_conn() else {
        tracing::warn!("bayes train: no network kevy connection");
        return;
    };
    mailrs_triage::spam::train(&mut conn, thread_id, &raws, is_spam);
}

/// A reader filed `thread_id` into or out of Junk: train on it, and record
/// the verdict as a screening observation. Only a reader's own action
/// comes here; the corpus bootstrap trains without recording.
pub fn learn_reader_verdict(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    is_spam: bool,
) {
    let raws = fetch_thread_raw(state, user, thread_id);
    if raws.is_empty() {
        return;
    }
    let Some(mut conn) = state.net_conn() else {
        tracing::warn!("bayes train: no network kevy connection");
        return;
    };
    mailrs_triage::spam::train(&mut conn, thread_id, &raws, is_spam);
    let now = chrono::Utc::now().timestamp();
    mailrs_core_sidestate::families::screening::learn_junk_verdict(
        &mut conn, user, thread_id, &raws, is_spam, now,
    );
}

/// v2.9 triage — train the shared multi-class triage corpus on a
/// thread the user (or the backfill) assigned to `class` ∈ {inbox,
/// notification, promotion}. Tokenizes the thread's maildir messages
/// and delegates the reversible corpus write to `mailrs-triage` (the
/// shared stone both serving lanes call). Best-effort.
pub fn train_triage(state: &Arc<FastcoreState>, user: &str, thread_id: &str, class: &str) {
    if !mailrs_triage::is_class(class) {
        return;
    }
    let raws = fetch_thread_raw(state, user, thread_id);
    if raws.is_empty() {
        return;
    }
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    mailrs_triage::spam::train_class(&mut conn, thread_id, &raws, class);
}

/// v2.9 triage — classify a raw message into a triage bucket category
/// ("inbox" | "notification" | "promotion") using the shared corpus.
/// Returns `None` on cold-start / low confidence — the caller then
/// defaults to "inbox".
pub fn classify_triage(state: &Arc<FastcoreState>, raw: &[u8]) -> Option<&'static str> {
    let mut conn = state.net_conn()?;
    mailrs_triage::classify(&mut conn, raw)
}

/// True if the corpus already holds any trained messages. The caller
/// (route handler) checks this ONCE before looping over accounts — a
/// per-account guard would let the first account's training lock out
/// every later account (v2.8.0 bug).
pub fn corpus_populated(state: &Arc<FastcoreState>) -> bool {
    let Some(mut conn) = state.net_conn() else {
        return false;
    };
    let meta = mailrs_triage::spam::keys(mailrs_triage::spam::READ).meta;
    let Ok(replies) = conn.pipeline(|p| {
        p.cmd(&[b"HGET", meta, b"spam_msgs"]);
        p.cmd(&[b"HGET", meta, b"ham_msgs"]);
    }) else {
        return false;
    };
    replies.iter().any(|r| {
        matches!(r, kevy_client::Reply::Bulk(b)
            if std::str::from_utf8(b).ok().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) > 0)
    })
}

/// One-shot corpus seed. Spam = the conversations this reader filed as
/// Junk and has not taken out ([`user_junked_threads`]), and the ones the
/// fraud rules hold. Never the server's own Junk (`category = spam`): that
/// is what the score decided, and a corpus seeded from it learns to agree
/// with the score, newsletters and all.
/// Ham = an equal count of the most-recent Inbox threads.
///
/// No per-account guard — the caller gates on [`corpus_populated`]
/// once. Returns `(spam_trained, ham_trained)`.
///
/// [`user_junked_threads`]: mailrs_core_sidestate::families::junk_corrections::user_junked_threads
pub fn bootstrap(state: &Arc<FastcoreState>, user: &str) -> (u64, u64) {
    let spam_ids = judged_spam(state, user);

    // Ham sample: same count, most-recent inbox threads.
    let want_ham = spam_ids.len() as i64;
    // Inbox is the declared bucket minus threads the user only ever sent —
    // the same definition the mail list uses, so the ham sample is drawn
    // from what the user actually sees as Inbox.
    let ham_ids: Vec<String> = if want_ham > 0 {
        state
            .mailbox
            .list_thread_ids_by_bucket_unsent_via_table(
                user,
                "inbox",
                mailrs_mailbox_kevy::ArchiveScope::All,
                want_ham as usize,
            )
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut spam_trained = 0u64;
    for tid in &spam_ids {
        train_thread(state, user, tid, true);
        spam_trained += 1;
    }
    let mut ham_trained = 0u64;
    for tid in &ham_ids {
        train_thread(state, user, tid, false);
        ham_trained += 1;
    }
    tracing::info!(%user, spam_trained, ham_trained, "bayes bootstrap complete");
    (spam_trained, ham_trained)
}

/// What may be learnt as spam for `user`: their own Junk filings and their
/// held conversations, sorted, each once.
pub(crate) fn judged_spam(state: &Arc<FastcoreState>, user: &str) -> Vec<String> {
    use mailrs_mailbox_kevy::{ListThreadsFilter, QuarantineScope};
    let mut ids: std::collections::BTreeSet<String> = state
        .net_conn()
        .and_then(|mut c| mailrs_core_sidestate::families::screening::observations(&mut c).ok())
        .map(|all| {
            mailrs_core_sidestate::families::junk_corrections::user_junked_threads(&all, user)
        })
        .unwrap_or_default();
    let held = ListThreadsFilter {
        quarantine: QuarantineScope::Only,
        ..Default::default()
    };
    if let Ok((rows, _)) = state
        .mailbox
        .list_threads_by_activity(user, &held, 0, 1 << 30)
    {
        ids.extend(rows.into_iter().map(|r| r.thread_id));
    }
    ids.into_iter().collect()
}

/// Header-heuristic seed label for a raw message — the one-time
/// bootstrap classifier that gives the learning corpus a starting
/// point (there is no historic N/P label data). Reuses the bayes
/// tokenizer's header-signal tokens:
///   - automated sender / Auto-Submitted → notification (transactional)
///   - List-Unsubscribe / List-Id (and not automated) → promotion
///   - else → inbox
///
/// Deliberately simple; the Bayes classifier refines it from the
/// user's mark-as corrections thereafter.
pub(crate) fn heuristic_triage_label(raw: &[u8]) -> &'static str {
    let toks = mailrs_parse_worker::tokens(
        raw,
        mailrs_parse_worker::Tokenizer::Decoded,
        mailrs_parse_worker::Limit::Batch,
    )
    .read()
    .unwrap_or_default();
    let has = |t: &str| toks.iter().any(|x| x == t);
    if has("from:automated") || has("hdr:auto-submitted") {
        "notification"
    } else if has("hdr:list-unsub") || has("hdr:list-id") {
        "promotion"
    } else {
        "inbox"
    }
}

/// v2.9 triage — one-shot backfill/seed. Walks the account's Inbox
/// threads, header-heuristic-labels each into inbox/notification/
/// promotion, re-files N/P out of Inbox (`set_bucket`), and trains the
/// multi-class corpus on every thread (all three classes, so
/// one-vs-rest has data for each). Returns `(inbox, notification,
/// promotion)` counts. Idempotent: `train_triage` is reversible and
/// `set_bucket` is a move.
pub fn backfill_triage(state: &Arc<FastcoreState>, user: &str) -> (u64, u64, u64) {
    use mailrs_mailbox_kevy::keys::Bucket;
    let tids: Vec<String> = state
        .mailbox
        .list_thread_ids_by_bucket_unsent_via_table(
            user,
            "inbox",
            mailrs_mailbox_kevy::ArchiveScope::All,
            ALL,
        )
        .unwrap_or_default();

    let (mut n_inbox, mut n_notif, mut n_promo) = (0u64, 0u64, 0u64);
    for tid in &tids {
        let raws = fetch_thread_raw(state, user, tid);
        if raws.is_empty() {
            continue;
        }
        let label = heuristic_triage_label(&raws[0]);
        match label {
            "notification" => {
                let _ = state.mailbox.set_bucket(user, tid, Bucket::Notifications);
                n_notif += 1;
            }
            "promotion" => {
                let _ = state.mailbox.set_bucket(user, tid, Bucket::Promotions);
                n_promo += 1;
            }
            _ => {
                n_inbox += 1;
            }
        }
        train_triage(state, user, tid, label);
    }
    tracing::info!(%user, n_inbox, n_notif, n_promo, "triage backfill complete");
    (n_inbox, n_notif, n_promo)
}

/// Read every raw message file backing a thread. Resolves each
/// message's `blob_ref` (from the wire blob) to a maildir path and
/// reads the bytes. Empty vec on any miss — training just skips.
pub(crate) fn fetch_thread_raw(state: &FastcoreState, user: &str, thread_id: &str) -> Vec<Vec<u8>> {
    let Some((local, domain)) = user.split_once('@') else {
        return Vec::new();
    };
    let base = std::path::PathBuf::from(&state.maildir_root)
        .join(domain)
        .join(local);

    let wires = match state.mailbox.list_thread_messages(user, thread_id) {
        Ok(w) => w,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for blob in wires {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&blob) else {
            continue;
        };
        let Some(blob_ref) = v.get("blob_ref").and_then(|x| x.as_str()) else {
            continue;
        };
        if let Some(bytes) = read_maildir_file(&base, blob_ref) {
            out.push(bytes);
        }
    }
    out
}

/// Read one maildir file by its `blob_ref`. A message once read has its
/// flags in the file name (`<ref>:2,S`), which only `locate` matches; a
/// lookup by the bare name found no file for any read message, so a thread
/// taken out of Junk was neither learnt from nor had its images fetched.
fn read_maildir_file(base: &std::path::Path, blob_ref: &str) -> Option<Vec<u8>> {
    let (dir, id) = mailrs_maildir::locate(base, blob_ref)?;
    dir.fetch(&id).ok().flatten()
}
