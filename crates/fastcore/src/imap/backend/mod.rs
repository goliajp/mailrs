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

use std::path::PathBuf;
use std::sync::Arc;

use mailrs_maildir::{Entry, Flag, Maildir};

use crate::FastcoreState;

mod flags;
mod modseq;

pub(crate) use flags::*;
pub(crate) use modseq::*;

/// A logical IMAP mailbox — a user's INBOX or Maildir++ subfolder.
#[derive(Debug, Clone)]
pub struct MailboxInfo {
    /// IMAP mailbox name (`INBOX`, `Sent`, `Drafts`, etc — no leading dot).
    pub name: String,
    /// Absolute disk path to the Maildir root for this mailbox.
    pub path: PathBuf,
}

/// A message row served to the IMAP session.
#[derive(Debug, Clone)]
pub struct ImapMessage {
    /// Persistent UID from the per-user allocator (same uid space the
    /// web API / message wires use — keyed by Message-ID, cached per
    /// maildir filename in kevy). Stable across sessions and restarts;
    /// never reused after expunge (the allocator is monotonic).
    pub uid: u32,
    /// Sequence number (1-based) in the current mailbox view. Follows
    /// same session-lifetime rules as `uid`.
    pub seqno: u32,
    /// Absolute path to the Maildir file (cur/ or new/).
    pub path: PathBuf,
    /// Flags derived from the Maildir filename info section.
    pub flags: Vec<Flag>,
    /// Timestamp component of the Maildir filename (unix seconds).
    /// Used for INTERNALDATE. Falls back to file mtime.
    pub internal_date: i64,
    /// File size in bytes.
    pub size: u64,
    /// CONDSTORE modification sequence (RFC 7162). 1 for messages that
    /// were never flag-mutated since tracking began.
    pub modseq: u64,
}

/// Verify a password against the hash stored in the account blob, by the one
/// sign-in check. Returns `false` on any parse / IO failure — never leaks the
/// reason.
pub fn verify_password(state: &FastcoreState, user: &str, password: &str) -> bool {
    let Ok(Some(blob)) = state.mailbox.get_account_blob(user) else {
        // no account: the same argon2 work, so timing does not tell
        mailrs_core::users::dummy_verify(password);
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&blob) else {
        return false;
    };
    let Some(hash_str) = v.get("password_hash").and_then(|h| h.as_str()) else {
        return false;
    };
    mailrs_auth_guard::password::matches(password, hash_str)
}

/// Discover every mailbox a user owns: INBOX plus every Maildir++
/// subfolder (`.<name>/`). Order is deterministic — INBOX first, then
/// alphabetical.
pub fn list_mailboxes(state: &Arc<FastcoreState>, user: &str) -> Vec<MailboxInfo> {
    let _ = state; // account existence is verified at LOGIN time
    list_mailboxes_in(&maildir_root(), user)
}

/// `MAILRS_MAILDIR`, the directory holding every user's maildir.
pub fn maildir_root() -> PathBuf {
    PathBuf::from(std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into()))
}

/// [`list_mailboxes`] under `root` rather than `MAILRS_MAILDIR`.
pub fn list_mailboxes_in(root: &std::path::Path, user: &str) -> Vec<MailboxInfo> {
    let Some((local, domain)) = user.split_once('@') else {
        return Vec::new();
    };
    let base = root.join(domain).join(local);
    let mut out = vec![MailboxInfo {
        name: "INBOX".into(),
        path: base.clone(),
    }];
    let Ok(iter) = std::fs::read_dir(&base) else {
        return out;
    };
    let mut subs: Vec<MailboxInfo> = Vec::new();
    for entry in iter.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') || name == "." || name == ".." {
            continue;
        }
        // Maildir++ hides "reserved" dirs — cur / new / tmp
        // themselves are never subfolders because they're not
        // dot-prefixed. `..` / `.` filtered above. Anything else that
        // has its own {cur,new,tmp} triplet counts as a mailbox.
        let sub_path = base.join(&name);
        if !sub_path.join("new").is_dir() {
            continue;
        }
        // IMAP name = subfolder without leading dot; nested dots
        // preserved so `.Work.Client` shows as `Work.Client`.
        let imap_name = name.trim_start_matches('.').to_string();
        subs.push(MailboxInfo {
            name: imap_name,
            path: sub_path,
        });
    }
    subs.sort_by(|a, b| a.name.cmp(&b.name));
    out.extend(subs);
    out
}

/// Look up a mailbox under `root` by IMAP name (`INBOX`, `Sent`,
/// `Work.Client`). Case-insensitive on `INBOX` per RFC 3501; the rest
/// match verbatim.
pub fn get_mailbox_in(root: &std::path::Path, user: &str, name: &str) -> Option<MailboxInfo> {
    let all = list_mailboxes_in(root, user);
    if name.eq_ignore_ascii_case("INBOX") {
        return all.into_iter().next();
    }
    all.into_iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Scan a mailbox and return every message ordered by ascending
/// persistent UID. seqno is the 1-based position in that order.
pub fn list_messages(state: &Arc<FastcoreState>, user: &str, mb: &MailboxInfo) -> Vec<ImapMessage> {
    let dir = Maildir::open(&mb.path);
    let mut entries: Vec<Entry> = Vec::new();
    if let Ok(new_entries) = dir.scan_new() {
        entries.extend(new_entries);
    }
    if let Ok(cur_entries) = dir.scan_cur() {
        entries.extend(cur_entries);
    }
    entries.sort_by(|a, b| a.id.0.cmp(&b.id.0));

    let mck = modseq_cache_key(user);
    let modseqs: std::collections::HashMap<String, u64> = state
        .mailbox
        .store_ref()
        .hgetall(mck.as_bytes())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            Some((
                String::from_utf8(k).ok()?,
                std::str::from_utf8(&v).ok()?.parse().ok()?,
            ))
        })
        .collect();

    // one HGETALL up front — subsequent SELECTs are cache hits only
    let ck = uid_cache_key(user);
    let cache: std::collections::HashMap<String, u32> = state
        .mailbox
        .store_ref()
        .hgetall(ck.as_bytes())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            Some((
                String::from_utf8(k).ok()?,
                std::str::from_utf8(&v).ok()?.parse().ok()?,
            ))
        })
        .collect();

    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let modseq = modseqs.get(&e.id.0).copied().unwrap_or(1);
        let uid = resolve_uid(state, user, &cache, &seen, &e.id.0, &e.path);
        if uid != 0 {
            seen.insert(uid);
        }
        let size = std::fs::metadata(&e.path).map(|m| m.len()).unwrap_or(0);
        let internal_date =
            e.id.0
                .split('.')
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|n| *n > 946_684_800)
                .or_else(|| {
                    std::fs::metadata(&e.path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                })
                .unwrap_or(0);
        out.push(ImapMessage {
            uid,
            seqno: 0, // assigned after the uid sort below
            path: e.path,
            flags: e.flags,
            internal_date,
            size,
            modseq,
        });
    }
    // RFC 3501: uids must be ascending with sequence order
    out.sort_by_key(|m| m.uid);
    for (idx, m) in out.iter_mut().enumerate() {
        m.seqno = (idx + 1) as u32;
    }
    out
}

/// Read the raw bytes of a message. Returns `None` when the file is
/// gone (expunged / moved / deleted since the scan).
pub fn read_message(msg: &ImapMessage) -> Option<Vec<u8>> {
    std::fs::read(&msg.path).ok()
}
