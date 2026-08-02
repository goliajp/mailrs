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

use std::path::PathBuf;
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

/// Move a message file to `dest_mailbox` (COPY / MOVE targets).
/// Creates the destination Maildir if it doesn't already exist.
///
/// Indexes the new copy on the way in — same write-through contract as
/// [`append`]. A COPY that only touched the filesystem left the thread
/// index blind to the new file.
pub fn copy_to(
    state: &Arc<FastcoreState>,
    user: &str,
    msg: &ImapMessage,
    dest: &MailboxInfo,
) -> std::io::Result<()> {
    let bytes = read_message(msg).ok_or_else(|| std::io::Error::other("source missing"))?;
    let maildir = Maildir::create(&dest.path)?;
    let id = maildir.deliver(&bytes)?;
    crate::ingest_delivered_file(state, user, &blob_ref_for(dest, &id.0), &bytes, &dest.name);
    Ok(())
}

/// Delete a message file — used by APPEND rollback + MOVE completion +
/// EXPUNGE.
pub fn delete_file(msg: &ImapMessage) -> std::io::Result<()> {
    std::fs::remove_file(&msg.path)
}

/// Append raw bytes to a mailbox as a new message; allocates and
/// returns the persistent UID for the delivered file.
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
    // Write-through to the thread index, exactly like spool_drain does
    // for inbound mail. Without this an APPEND (a client filing its own
    // sent copy into .Sent, say) lands on disk but stays invisible to
    // the conversation views until the periodic maildir self-heal
    // notices it — which is precisely why that sweep had to scan every
    // file in the mailbox on every cycle (2026-07-19).
    crate::ingest_delivered_file(state, user, &blob_ref_for(mb, &id.0), bytes, &mb.name);
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

        append(&st, "u@x.y", &m, raw).unwrap();

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
