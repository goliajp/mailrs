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

/// Kevy hash caching maildir base-filename → uid per user. The uid
/// values come from `allocate_uid` (keyed by Message-ID), i.e. the SAME
/// per-user uid space message wires and the web API use.
fn uid_cache_key(user: &str) -> String {
    format!("mailrs:user:{user}:imap:uid_by_file")
}

/// Resolve the persistent uid for one maildir file, consulting /
/// filling the per-user cache. `seen` guards RFC 3501 uid uniqueness
/// within one mailbox scan: if two files claim the same uid (same
/// Message-ID copied twice into one folder), the second falls back to
/// a filename-keyed allocation.
fn resolve_uid(
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

/// Persistent UIDVALIDITY for one (user, mailbox). Allocated from the
/// boot epoch on first SELECT and never changed afterwards — clients
/// may cache uids forever.
pub fn uidvalidity(state: &Arc<FastcoreState>, user: &str, mailbox_name: &str) -> u32 {
    let key = format!("mailrs:user:{user}:imap:uidvalidity:{mailbox_name}");
    if let Ok(Some(v)) = state.mailbox.store_ref().get(key.as_bytes())
        && let Ok(s) = std::str::from_utf8(&v)
        && let Ok(n) = s.parse::<u32>()
    {
        return n;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(1)
        .max(1);
    let _ = state
        .mailbox
        .store_ref()
        .set(key.as_bytes(), now.to_string().as_bytes());
    now
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
fn modseq_cache_key(user: &str) -> String {
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

/// Record an expunged uid for QRESYNC `VANISHED (EARLIER)` replay
/// (RFC 7162 §3.2.9). Scored by the modseq at expunge time.
pub fn record_vanished(
    state: &Arc<FastcoreState>,
    user: &str,
    folder: &str,
    uid: u32,
    modseq: u64,
) {
    let key = format!("mailrs:user:{user}:imap:vanished:{folder}");
    let _ = state.mailbox.store_ref().zadd(
        key.as_bytes(),
        &[(modseq as f64, uid.to_string().as_bytes())],
    );
}

/// Uids expunged after `since` (exclusive), ascending.
pub fn vanished_since(
    state: &Arc<FastcoreState>,
    user: &str,
    folder: &str,
    since: u64,
) -> Vec<u32> {
    let key = format!("mailrs:user:{user}:imap:vanished:{folder}");
    let mut uids: Vec<u32> = state
        .mailbox
        .store_ref()
        .zrange_by_score(key.as_bytes(), (since + 1) as f64, f64::MAX)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(m, _score)| String::from_utf8(m).ok()?.parse().ok())
        .collect();
    uids.sort_unstable();
    uids
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

/// Append raw bytes to a mailbox as a new message; allocates and
/// returns the persistent UID for the delivered file.
pub fn append(
    state: &Arc<FastcoreState>,
    user: &str,
    mb: &MailboxInfo,
    bytes: &[u8],
) -> std::io::Result<u32> {
    if crate::live_sync::quota_exceeded(user) {
        return Err(std::io::Error::other("over quota"));
    }
    let maildir = Maildir::create(&mb.path)?;
    let id = maildir.deliver(bytes)?;
    crate::live_sync::adjust_usage_bytes(user, bytes.len() as i64);
    let empty_cache = std::collections::HashMap::new();
    let empty_seen = std::collections::HashSet::new();
    let path = mb.path.join("new").join(&id.0);
    let m = bump_modseq(state, user);
    set_file_modseq(state, user, &id.0, m);
    Ok(resolve_uid(
        state,
        user,
        &empty_cache,
        &empty_seen,
        &id.0,
        &path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FastcoreState;
    use kevy_embedded::{Config, Store};
    use mailrs_mailbox_kevy::KevyMailboxStore;

    fn state() -> Arc<FastcoreState> {
        let store = Arc::new(Store::open(Config::default()).expect("mem store"));
        Arc::new(FastcoreState::new(KevyMailboxStore::new(store)))
    }

    fn mb(dir: &std::path::Path) -> MailboxInfo {
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(dir.join(leaf)).unwrap();
        }
        MailboxInfo {
            name: "INBOX".into(),
            path: dir.to_path_buf(),
        }
    }

    fn write_msg(dir: &std::path::Path, name: &str, mid: &str) {
        std::fs::write(
            dir.join("new").join(name),
            format!("Message-ID: <{mid}>\r\nSubject: t\r\n\r\nbody"),
        )
        .unwrap();
    }

    #[test]
    fn uids_stable_across_rescans_and_ascending() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state();
        let m = mb(tmp.path());
        write_msg(tmp.path(), "1700000001.M1P1.h", "a@test");
        write_msg(tmp.path(), "1700000002.M2P1.h", "b@test");
        let first = list_messages(&st, "u@x.y", &m);
        assert_eq!(first.len(), 2);
        assert!(first[0].uid < first[1].uid, "ascending uids");
        assert_eq!(first[0].seqno, 1);
        // rescan — uids must be identical (cache hit)
        let second = list_messages(&st, "u@x.y", &m);
        assert_eq!(
            first.iter().map(|m| m.uid).collect::<Vec<_>>(),
            second.iter().map(|m| m.uid).collect::<Vec<_>>()
        );
        // new arrival gets a strictly higher uid
        write_msg(tmp.path(), "1700000003.M3P1.h", "c@test");
        let third = list_messages(&st, "u@x.y", &m);
        assert_eq!(third.len(), 3);
        assert!(third[2].uid > second[1].uid);
    }

    #[test]
    fn uid_shared_with_wire_allocator_by_message_id() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state();
        // the deliver path allocated a uid for this Message-ID first
        let wire_uid = st.mailbox.allocate_uid("u@x.y", "a@test").unwrap();
        let m = mb(tmp.path());
        write_msg(tmp.path(), "1700000001.M1P1.h", "a@test");
        let msgs = list_messages(&st, "u@x.y", &m);
        assert_eq!(msgs[0].uid, wire_uid, "IMAP and web API agree on the uid");
    }

    #[test]
    fn duplicate_message_id_in_one_mailbox_gets_distinct_uids() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state();
        let m = mb(tmp.path());
        write_msg(tmp.path(), "1700000001.M1P1.h", "same@test");
        write_msg(tmp.path(), "1700000002.M2P1.h", "same@test");
        let msgs = list_messages(&st, "u@x.y", &m);
        assert_eq!(msgs.len(), 2);
        assert_ne!(msgs[0].uid, msgs[1].uid, "RFC 3501 uid uniqueness");
    }

    #[test]
    fn uidvalidity_persists() {
        let st = state();
        let v1 = uidvalidity(&st, "u@x.y", "INBOX");
        let v2 = uidvalidity(&st, "u@x.y", "INBOX");
        assert_eq!(v1, v2);
        assert!(v1 > 1, "epoch-derived, not the old hardcoded 1");
    }
}
