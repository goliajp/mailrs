//! Mutations: flags, copy, delete, append, and the vanished log.

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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_maildir::{Flag, Maildir, MessageId};

use super::*;
use crate::FastcoreState;

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

/// Write `bytes` into the Maildir at `dir` as a new file carrying `flags`
/// in its name. Returns the file's id and path.
fn deliver_flagged(
    dir: &Path,
    bytes: &[u8],
    flags: &[Flag],
) -> std::io::Result<(MessageId, PathBuf)> {
    let maildir = Maildir::create(dir)?;
    let id = maildir.deliver(bytes)?;
    if flags.is_empty() {
        let path = dir.join("new").join(&id.0);
        return Ok((id, path));
    }
    maildir.mark_processed(&id, flags)?;
    let name = format!("{}{}", id.0, mailrs_maildir::serialize_flags(flags));
    let path = dir.join("cur").join(name);
    Ok((id, path))
}

/// Copy a message file into `dest` (COPY / MOVE targets), with the flags it
/// has: a message read before it was filed is still read after.
///
/// Indexes the new copy on the way in — same write-through contract as
/// [`append`]. A COPY that only touched the filesystem left the thread
/// index blind to the new file.
///
/// Returns the uid the copy has in `dest`.
pub fn copy_to(
    state: &Arc<FastcoreState>,
    user: &str,
    msg: &ImapMessage,
    dest: &MailboxInfo,
) -> std::io::Result<u32> {
    let bytes = read_message(msg).ok_or_else(|| std::io::Error::other("source missing"))?;
    let (id, path) = deliver_flagged(&dest.path, &bytes, &msg.flags)?;
    crate::ingest_delivered_file(
        state,
        user,
        &blob_ref_for(dest, &id.0),
        &bytes,
        &dest.name,
        None,
    );
    Ok(new_file_uid(state, user, &id.0, &path))
}

/// The uid of a file just delivered, from the allocator every reader of
/// the mailbox resolves it with.
fn new_file_uid(state: &Arc<FastcoreState>, user: &str, base: &str, path: &Path) -> u32 {
    let empty_cache = std::collections::HashMap::new();
    let empty_seen = std::collections::HashSet::new();
    resolve_uid(state, user, &empty_cache, &empty_seen, base, path)
}

/// Remove a message file and take its size off the user's usage, the one
/// place a removal is counted. Arrivals are counted where they are
/// indexed (`ingest_delivered_file`).
pub fn remove_message(user: &str, msg: &ImapMessage) -> std::io::Result<()> {
    std::fs::remove_file(&msg.path)?;
    crate::live_sync::adjust_usage_bytes(user, -(msg.size as i64));
    Ok(())
}

/// A copy in `dest`, then the source removed: the user's usage ends where
/// it started. Returns the uid the message has in `dest`.
pub fn move_to(
    state: &Arc<FastcoreState>,
    user: &str,
    msg: &ImapMessage,
    dest: &MailboxInfo,
) -> std::io::Result<u32> {
    let uid = copy_to(state, user, msg, dest)?;
    remove_message(user, msg)?;
    Ok(uid)
}

/// Build the `blob_ref` that `ingest_delivered_file` (and later
/// `enrich_with_body`) expects: a bare filename for INBOX, or
/// `.Folder/<filename>` for a Maildir++ subfolder. Mirrors the
/// construction in `healed_from_maildir`.
///
/// Keyed off the IMAP mailbox name rather than the directory name:
/// `list_mailboxes` derives the name by stripping the leading dot
/// (`.Work.Client` -> `Work.Client`), so re-adding it round-trips
/// exactly, and the result doesn't depend on what the maildir root
/// itself happens to be called.
pub(crate) fn blob_ref_for(mb: &MailboxInfo, filename: &str) -> String {
    match mb.name.eq_ignore_ascii_case("INBOX") {
        true => filename.to_string(),
        false => format!(".{}/{}", mb.name, filename),
    }
}

/// Append raw bytes to a mailbox as a new message, allocating its
/// persistent uid. `flags` are written into the file name, so the message
/// arrives carrying them; bringing the index in step is the caller's.
/// Returns the uid and the file.
pub fn append(
    state: &Arc<FastcoreState>,
    user: &str,
    mb: &MailboxInfo,
    bytes: &[u8],
    flags: &[Flag],
) -> std::io::Result<(u32, PathBuf)> {
    if crate::live_sync::quota_exceeded(user) {
        return Err(std::io::Error::other("over quota"));
    }
    let (id, path) = deliver_flagged(&mb.path, bytes, flags)?;
    // Write-through to the thread index, which also counts the bytes
    // against the user's usage, exactly like spool_drain does
    // for inbound mail. Without this an APPEND (a client filing its own
    // sent copy into .Sent, say) lands on disk but stays invisible to
    // the conversation views until the periodic maildir self-heal
    // notices it — which is precisely why that sweep had to scan every
    // file in the mailbox on every cycle (2026-07-19).
    crate::ingest_delivered_file(state, user, &blob_ref_for(mb, &id.0), bytes, &mb.name, None);
    let m = bump_modseq(state, user);
    set_file_modseq(state, user, &id.0, m);
    Ok((new_file_uid(state, user, &id.0, &path), path))
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
    fn blob_ref_is_bare_for_inbox_and_prefixed_for_subfolders() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = mb(tmp.path());
        assert_eq!(blob_ref_for(&inbox, "123.M1P1.h"), "123.M1P1.h");

        let sub_path = tmp.path().join(".Sent");
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(sub_path.join(leaf)).unwrap();
        }
        let sent = MailboxInfo {
            name: "Sent".into(),
            path: sub_path,
        };
        assert_eq!(blob_ref_for(&sent, "123.M1P1.h"), ".Sent/123.M1P1.h");
    }

    #[test]
    fn append_indexes_the_message_write_through() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state();
        let m = mb(tmp.path());
        let raw = b"Message-ID: <appended@test>\r\nFrom: a@x.y\r\nSubject: hi\r\n\r\nbody";

        append(&st, "u@x.y", &m, raw, &[]).unwrap();

        // the thread index must know about it immediately — no waiting
        // for the periodic maildir self-heal
        let tid = st
            .mailbox
            .thread_for_message_id("u@x.y", "appended@test")
            .unwrap();
        assert!(
            tid.is_some(),
            "APPEND must write through to the thread index"
        );
    }

    #[test]
    fn a_copy_keeps_the_flags_the_message_had() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state();
        let inbox = mb(tmp.path());
        let raw = b"Message-ID: <read@test>\r\nSubject: hi\r\n\r\nbody";
        let flags = [Flag::Seen, Flag::Flagged];
        let (uid, path) = append(&st, "u@x.y", &inbox, raw, &flags).unwrap();
        let trash = MailboxInfo {
            name: "Trash".into(),
            path: tmp.path().join(".Trash"),
        };
        let msg = ImapMessage {
            uid,
            seqno: 1,
            path,
            flags: flags.to_vec(),
            internal_date: 0,
            size: raw.len() as u64,
            modseq: 0,
        };

        copy_to(&st, "u@x.y", &msg, &trash).unwrap();

        let copies = list_messages(&st, "u@x.y", &trash);
        assert_eq!(copies.len(), 1);
        let kept = flags.iter().all(|f| copies[0].flags.contains(f));
        assert!(
            kept,
            "a read message copied is still read: {:?}",
            copies[0].flags
        );
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
