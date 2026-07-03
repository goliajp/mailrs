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

use mailrs_maildir::{Entry, Flag, Maildir, MessageId};

use crate::FastcoreState;

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
    /// Stable UID assigned by [`ImapBackend::list_messages`]. Kept as
    /// the file's ordinal position in the current scan; the fastcore
    /// listener rescans on every SELECT so UIDs are stable for the
    /// duration of a single session.
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
}

/// Verify a plaintext password against the argon2 hash stored in the
/// account blob. Returns `false` on any parse / IO failure — never
/// leaks the reason (constant-time semantics via argon2 verify).
pub fn verify_password(state: &FastcoreState, user: &str, password: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let Ok(Some(blob)) = state.mailbox.get_account_blob(user) else {
        // No account — still do a dummy hash so timing looks the same
        // as the real path. Reuses the argon2 default cost.
        let _ = Argon2::default().verify_password(
            password.as_bytes(),
            &PasswordHash::new(
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$WkBqROaBGeYaGmCzR1r7Dq7d",
            )
            .unwrap_or_else(|_| PasswordHash::new("").expect("empty hash")),
        );
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&blob) else {
        return false;
    };
    let Some(hash_str) = v.get("password_hash").and_then(|h| h.as_str()) else {
        return false;
    };
    let Ok(parsed) = PasswordHash::new(hash_str) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Discover every mailbox a user owns: INBOX plus every Maildir++
/// subfolder (`.<name>/`). Order is deterministic — INBOX first, then
/// alphabetical.
pub fn list_mailboxes(state: &Arc<FastcoreState>, user: &str) -> Vec<MailboxInfo> {
    let _ = state; // account existence is verified at LOGIN time
    let Some((local, domain)) = user.split_once('@') else {
        return Vec::new();
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = PathBuf::from(&root).join(domain).join(local);
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

/// Look up a mailbox by IMAP name (`INBOX`, `Sent`, `Work.Client`).
/// Case-insensitive on `INBOX` per RFC 3501; the rest match verbatim.
pub fn get_mailbox(state: &Arc<FastcoreState>, user: &str, name: &str) -> Option<MailboxInfo> {
    let all = list_mailboxes(state, user);
    if name.eq_ignore_ascii_case("INBOX") {
        return all.into_iter().next();
    }
    all.into_iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Scan a mailbox and return every message ordered by delivery epoch
/// (Maildir filename timestamp). UID + seqno both start at 1.
pub fn list_messages(mb: &MailboxInfo) -> Vec<ImapMessage> {
    let dir = Maildir::open(&mb.path);
    let mut entries: Vec<Entry> = Vec::new();
    if let Ok(new_entries) = dir.scan_new() {
        entries.extend(new_entries);
    }
    if let Ok(cur_entries) = dir.scan_cur() {
        entries.extend(cur_entries);
    }
    entries.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    let mut out = Vec::with_capacity(entries.len());
    for (idx, e) in entries.into_iter().enumerate() {
        let uid = (idx + 1) as u32;
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
            seqno: uid,
            path: e.path,
            flags: e.flags,
            internal_date,
            size,
        });
    }
    out
}

/// Read the raw bytes of a message. Returns `None` when the file is
/// gone (expunged / moved / deleted since the scan).
pub fn read_message(msg: &ImapMessage) -> Option<Vec<u8>> {
    std::fs::read(&msg.path).ok()
}

/// Overwrite the flag suffix on a message file. Uses Maildir++ rename
/// semantics via the storage-maildir crate. `flags` is the new full
/// set; caller has already merged additions / removals.
pub fn set_flags(msg: &ImapMessage, flags: &[Flag]) -> std::io::Result<PathBuf> {
    let dir = msg.path.parent().and_then(|p| p.parent()).ok_or_else(|| {
        std::io::Error::other(format!("bad maildir path: {}", msg.path.display()))
    })?;
    let maildir = Maildir::open(dir);
    let id = MessageId(
        msg.path
            .file_name()
            .map(|f| {
                f.to_string_lossy()
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .unwrap_or_default(),
    );
    maildir.mark_processed(&id, flags)?;
    // storage-maildir moves the file into cur/ with the new info
    // suffix; find + return the new path.
    let cur_dir = dir.join("cur");
    let want_prefix = id.0.as_str();
    for entry in std::fs::read_dir(&cur_dir)?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(want_prefix) {
            return Ok(entry.path());
        }
    }
    Ok(msg.path.clone())
}

/// Move a message file to `dest_mailbox` (COPY / MOVE targets).
/// Creates the destination Maildir if it doesn't already exist.
pub fn copy_to(msg: &ImapMessage, dest: &MailboxInfo) -> std::io::Result<()> {
    let bytes = read_message(msg).ok_or_else(|| std::io::Error::other("source missing"))?;
    let maildir = Maildir::create(&dest.path)?;
    maildir.deliver(&bytes)?;
    Ok(())
}

/// Delete a message file — used by APPEND rollback + MOVE completion +
/// EXPUNGE.
pub fn delete_file(msg: &ImapMessage) -> std::io::Result<()> {
    std::fs::remove_file(&msg.path)
}

/// Append raw bytes to a mailbox as a new message; returns the new
/// UID (using the same ordinal scheme `list_messages` produces).
pub fn append(mb: &MailboxInfo, bytes: &[u8]) -> std::io::Result<u32> {
    let maildir = Maildir::create(&mb.path)?;
    maildir.deliver(bytes)?;
    Ok(list_messages(mb).len() as u32)
}
