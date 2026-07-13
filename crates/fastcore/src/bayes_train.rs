//! Bayesian spam-corpus training + storage, network-kevy backed.
//!
//! RFC `.claude/rfcs/20260713-bayes-antispam-engine.md`. The pure
//! math lives in the `mailrs-bayes` stone; this module is the
//! cement/steel glue: read a thread's raw messages from the maildir,
//! tokenize them, and HINCRBY the per-token spam/ham counts into the
//! shared network kevy so the receiver process (a different binary)
//! can classify at SMTP time.
//!
//! Storage keys (network kevy):
//!   bayes:tokens:spam   hash  field=token value=message-count
//!   bayes:tokens:ham    hash  field=token value=message-count
//!   bayes:meta          hash  {spam_msgs, ham_msgs}
//!   bayes:trained:<thread_id>  string  "spam"|"ham"  (TTL 90d)
//!     — records the last training direction so re-marking a thread
//!       (junk → not-junk) unlearns before it re-learns.

use std::sync::Arc;

use crate::FastcoreState;

const K_SPAM: &[u8] = b"bayes:tokens:spam";
const K_HAM: &[u8] = b"bayes:tokens:ham";
const K_META: &[u8] = b"bayes:meta";
const TRAINED_TTL_SECS: i64 = 90 * 24 * 3600;

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
    // Union of tokens across the thread's messages — one training
    // sample per thread (a thread is one spam/ham decision, not N).
    let mut tokens: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for raw in &raws {
        tokens.extend(mailrs_bayes::tokenize(raw));
    }
    if tokens.is_empty() {
        return;
    }

    let Some(mut conn) = state.net_conn() else {
        tracing::warn!("bayes train: no network kevy connection");
        return;
    };
    let trained_key = format!("bayes:trained:{thread_id}");

    // Reconcile prior training direction: read the last-trained marker.
    let prior = conn
        .get(trained_key.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok());
    let now_dir = if is_spam { "spam" } else { "ham" };
    if prior.as_deref() == Some(now_dir) {
        // Already trained this way — refresh TTL and return.
        set_trained_marker(&mut conn, thread_id, now_dir);
        return;
    }

    let token_list: Vec<&str> = tokens.iter().map(String::as_str).collect();

    // Unlearn the opposite direction if this thread was trained before.
    if let Some(prev) = prior.as_deref() {
        let prev_spam = prev == "spam";
        adjust_tokens(&mut conn, &token_list, prev_spam, -1);
        adjust_meta(&mut conn, prev_spam, -1);
    }

    // Learn the new direction.
    adjust_tokens(&mut conn, &token_list, is_spam, 1);
    adjust_meta(&mut conn, is_spam, 1);
    set_trained_marker(&mut conn, thread_id, now_dir);

    tracing::info!(
        %user,
        %thread_id,
        is_spam,
        tokens = token_list.len(),
        "bayes: trained thread"
    );
}

/// One-shot corpus seed from the existing folders (RFC §5). Spam =
/// every thread in the Junk folder; ham = an equal number of the most
/// recent Inbox threads (newsletter category excluded — legitimate
/// bulk would skew the ham profile). Refuses (returns None) if the
/// corpus is already non-empty, so a second call can't double-count.
///
/// Returns `Some((spam_trained, ham_trained))` on success.
pub fn bootstrap(state: &Arc<FastcoreState>, user: &str) -> Option<(u64, u64)> {
    // Guard: corpus must be empty.
    {
        let mut conn = state.net_conn()?;
        let meta = conn.pipeline(|p| {
            p.cmd(&[b"HGET", K_META, b"spam_msgs"]);
            p.cmd(&[b"HGET", K_META, b"ham_msgs"]);
        });
        if let Ok(replies) = meta {
            let any_nonzero = replies.iter().any(|r| {
                matches!(r, kevy_client::Reply::Bulk(b)
                    if std::str::from_utf8(b).ok().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) > 0)
            });
            if any_nonzero {
                return None;
            }
        }
    }

    let store = state.mailbox.store_ref();
    let junk_key = mailrs_mailbox_kevy::keys::user_threads_junk(user);
    let inbox_key = mailrs_mailbox_kevy::keys::user_threads_inbox(user);

    let junk_ids: Vec<String> = store
        .zrevrange(junk_key.as_bytes(), 0, -1)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(m, _)| String::from_utf8(m).ok())
        .collect();

    // Ham sample: same count as spam, most-recent inbox threads.
    let want_ham = junk_ids.len() as i64;
    let ham_ids: Vec<String> = if want_ham > 0 {
        store
            .zrevrange(inbox_key.as_bytes(), 0, want_ham - 1)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(m, _)| String::from_utf8(m).ok())
            .collect()
    } else {
        Vec::new()
    };

    let mut spam_trained = 0u64;
    for tid in &junk_ids {
        train_thread(state, user, tid, true);
        spam_trained += 1;
    }
    let mut ham_trained = 0u64;
    for tid in &ham_ids {
        train_thread(state, user, tid, false);
        ham_trained += 1;
    }
    tracing::info!(%user, spam_trained, ham_trained, "bayes bootstrap complete");
    Some((spam_trained, ham_trained))
}

/// HINCRBY every token in the given corpus hash by `delta` (+1 learn,
/// -1 unlearn). Clamps to >= 0 is NOT applied here — kevy has no
/// atomic clamp; a stray negative is harmless (classify treats counts
/// as u32 via saturating parse) and self-corrects on the next learn.
fn adjust_tokens(conn: &mut kevy_client::Connection, tokens: &[&str], spam: bool, delta: i64) {
    let key = if spam { K_SPAM } else { K_HAM };
    let delta_s = delta.to_string();
    let _ = conn.pipeline(|p| {
        for t in tokens {
            p.cmd(&[b"HINCRBY", key, t.as_bytes(), delta_s.as_bytes()]);
        }
    });
}

fn adjust_meta(conn: &mut kevy_client::Connection, spam: bool, delta: i64) {
    let field: &[u8] = if spam { b"spam_msgs" } else { b"ham_msgs" };
    let delta_s = delta.to_string();
    let _ = conn.pipeline(|p| {
        p.cmd(&[b"HINCRBY", K_META, field, delta_s.as_bytes()]);
    });
}

fn set_trained_marker(conn: &mut kevy_client::Connection, thread_id: &str, dir: &str) {
    let key = format!("bayes:trained:{thread_id}");
    let _ = conn.set(key.as_bytes(), dir.as_bytes());
    let _ = conn.expire(
        key.as_bytes(),
        std::time::Duration::from_secs(TRAINED_TTL_SECS as u64),
    );
}

/// Read every raw message file backing a thread. Resolves each
/// message's `blob_ref` (from the wire blob) to a maildir path and
/// reads the bytes. Empty vec on any miss — training just skips.
fn fetch_thread_raw(state: &Arc<FastcoreState>, user: &str, thread_id: &str) -> Vec<Vec<u8>> {
    let Some((local, domain)) = user.split_once('@') else {
        return Vec::new();
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(&root).join(domain).join(local);

    let wires = match state.mailbox.list_thread_messages(thread_id) {
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

/// Read one maildir file by its `blob_ref`. A ref with a `/` is a
/// Maildir++ subfolder path (`.Sent/<file>`); otherwise it's a bare
/// INBOX filename tried under `cur/` then `new/`.
fn read_maildir_file(base: &std::path::Path, blob_ref: &str) -> Option<Vec<u8>> {
    if let Some((sub, file)) = blob_ref.split_once('/') {
        for leaf in ["cur", "new"] {
            let p = base.join(sub).join(leaf).join(file);
            if let Ok(b) = std::fs::read(&p) {
                return Some(b);
            }
        }
        // Some refs already include cur/new in the subfolder segment.
        let p = base.join(sub).join(file);
        return std::fs::read(p).ok();
    }
    for leaf in ["cur", "new"] {
        let p = base.join(leaf).join(blob_ref);
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}
