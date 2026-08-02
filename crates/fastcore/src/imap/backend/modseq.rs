//! UID and MODSEQ bookkeeping.

//! Kevy + maildir backend for the fastcore IMAP server.
//!
//! Responsibilities:
//! - Verify LOGIN credentials against fastcore's embedded kevy account
//!   store (same hash format webapi uses).
//! - Enumerate a user's mailboxes from their maildir directory (INBOX
//!   plus every Maildir++ subfolder).
//! - Enumerate + fetch messages inside a mailbox.
//! - Persist per-message flag updates via Maildir++ filename info
//!   (`:2,SRF` etc — the storage-maildir crate handles the rename).

use std::sync::Arc;

use crate::FastcoreState;

/// Kevy hash caching maildir base-filename → uid per user. The uid
/// values come from `allocate_uid` (keyed by Message-ID), i.e. the SAME
/// per-user uid space message wires and the web API use.
pub(crate) fn uid_cache_key(user: &str) -> String {
    format!("mailrs:user:{user}:imap:uid_by_file")
}

/// Resolve the persistent uid for one maildir file, consulting /
/// filling the per-user cache. `seen` guards RFC 3501 uid uniqueness
/// within one mailbox scan: if two files claim the same uid (same
/// Message-ID copied twice into one folder), the second falls back to
/// a filename-keyed allocation.
pub(crate) fn resolve_uid(
    state: &Arc<FastcoreState>,
    user: &str,
    cache: &std::collections::HashMap<String, u32>,
    seen: &std::collections::HashSet<u32>,
    base: &str,
    path: &std::path::Path,
) -> u32 {
    if let Some(uid) = cache.get(base)
        && *uid != 0
        && !seen.contains(uid)
    {
        return *uid;
    }
    // miss (or intra-mailbox duplicate) — derive the allocation key
    let head = std::fs::read(path)
        .map(|b| b[..b.len().min(16 * 1024)].to_vec())
        .unwrap_or_default();
    let (message_id, ..) = crate::extract_headers(&head);
    let mut key = if message_id.is_empty() {
        format!("file:{base}")
    } else {
        message_id
    };
    let mut uid = state.mailbox.allocate_uid(user, &key).unwrap_or(0);
    if uid != 0 && seen.contains(&uid) {
        // duplicate Message-ID within this mailbox — force a distinct uid
        key = format!("file:{base}");
        uid = state.mailbox.allocate_uid(user, &key).unwrap_or(0);
    }
    if uid != 0 {
        let ck = uid_cache_key(user);
        let _ = state.mailbox.store_ref().hset(
            ck.as_bytes(),
            &[(base.as_bytes(), uid.to_string().as_bytes())],
        );
    }
    uid
}

/// Persistent UIDVALIDITY for one (user, mailbox). Allocated from the
/// boot epoch on first SELECT and never changed afterwards — clients
/// may cache uids forever.
pub fn uidvalidity(state: &Arc<FastcoreState>, user: &str, mailbox_name: &str) -> u32 {
    let key = format!("mailrs:user:{user}:imap:uidvalidity:{mailbox_name}");
    // v2 Stage B.2 · Phase 2: get + conditional-set collapsed into
    // one atomic closure. Prior implementation could race the initial
    // get miss with a concurrent first-select on the same mailbox —
    // both callers picked their own `now` and one raced ahead with
    // its stamp, leaving the loser's return value diverging from
    // what was persisted. Two IMAP clients briefly saw different
    // UIDVALIDITY values for the same mailbox.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(1)
        .max(1);
    state
        .mailbox
        .store_ref()
        .atomic(|ctx| {
            if let Some(v) = ctx.get(key.as_bytes())?
                && let Ok(s) = std::str::from_utf8(&v)
                && let Ok(n) = s.parse::<u32>()
            {
                return Ok(n);
            }
            ctx.set(key.as_bytes(), now.to_string().as_bytes());
            Ok(now)
        })
        .unwrap_or(now)
}

/// Predicted next uid — the per-user allocation counter + 1. Strictly
/// greater than every uid in every mailbox (per-user uid space).
pub fn uid_next(state: &Arc<FastcoreState>, user: &str) -> u32 {
    let key = mailrs_mailbox_kevy::keys::user_next_uid(user);
    let last: u32 = state
        .mailbox
        .store_ref()
        .get(key.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    last.saturating_add(1)
}

/// Kevy hash caching maildir base-filename → modseq per user.
pub(crate) fn modseq_cache_key(user: &str) -> String {
    format!("mailrs:user:{user}:imap:modseq_by_file")
}

/// Bump and return the per-user modification sequence (RFC 7162).
/// Monotonic across every mailbox the user owns — a legal (if coarse)
/// HIGHESTMODSEQ domain.
pub fn bump_modseq(state: &Arc<FastcoreState>, user: &str) -> u64 {
    let key = format!("mailrs:user:{user}:imap:modseq");
    // +1 bias: never-mutated messages default to modseq 1, so the very
    // first bump must land at 2 — a raw first incr() returns 1 and the
    // mutation becomes invisible to CHANGEDSINCE (caught on staging)
    state
        .mailbox
        .store_ref()
        .incr(key.as_bytes())
        .map(|v| (v.max(0) as u64) + 1)
        .unwrap_or(2)
}

/// Current highest modseq for the user (1 when never bumped).
pub fn highest_modseq(state: &Arc<FastcoreState>, user: &str) -> u64 {
    let key = format!("mailrs:user:{user}:imap:modseq");
    state
        .mailbox
        .store_ref()
        .get(key.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|c| c + 1)
        .unwrap_or(1)
        .max(1)
}

/// Record a message file's modseq after a mutation.
pub fn set_file_modseq(state: &Arc<FastcoreState>, user: &str, base: &str, modseq: u64) {
    let ck = modseq_cache_key(user);
    let _ = state.mailbox.store_ref().hset(
        ck.as_bytes(),
        &[(base.as_bytes(), modseq.to_string().as_bytes())],
    );
}
