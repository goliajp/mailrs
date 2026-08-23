//! Telling the other server what happened here.
//!
//! A connected mailbox is somebody's mailbox. Reading a message in
//! mailrs and finding it still bold in Gmail is the same defect as the
//! reverse — which is why `\Seen` now arrives on the way in — and it is
//! the more visible half: the other client is open on their phone.
//!
//! **Queued, not done inline.** The read path answers a person pressing
//! a key; it must not wait on an IMAP round trip to a server that may
//! be slow, rate-limiting or asleep. So a read leaves a note and the
//! sync worker, which is already connected to that account, carries it.
//!
//! Nothing here retries forever: a note whose account is gone, or whose
//! folder no longer holds that uid, is dropped rather than kept as a
//! job that can never finish.

use std::sync::Arc;

use crate::FastcoreState;

/// Where a message came from, read back out of its blob reference.
///
/// The sync writes `ext-{account}-{folder}-{uid}`, and that is the only
/// place the origin is recorded — the message row carries the blob
/// reference and nothing else about where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// Which connected account.
    pub account_id: String,
    /// The folder, as `sanitise` wrote it.
    pub folder: String,
    /// The uid **under the uidvalidity that was current when it was
    /// fetched**, which is why a note is dropped rather than retried
    /// when the folder has been renumbered since.
    pub uid: u32,
}

/// Read the origin out of a blob reference, or nothing.
///
/// Everything that did not come from a connected account returns
/// `None`, which is most mail: the spool writes maildir names.
///
/// The folder is between the first and last separators, and it may
/// contain them — `sanitise` maps every non-alphanumeric byte to `_`,
/// so `[Gmail]/Sent Mail` becomes `_Gmail__Sent_Mail`. Splitting from
/// both ends is what survives that.
pub fn origin_of(blob_ref: &str) -> Option<Origin> {
    let rest = blob_ref.strip_prefix("ext-")?;
    let (account_id, rest) = rest.split_once('-')?;
    let (folder, uid) = rest.rsplit_once('-')?;
    if account_id.is_empty() || folder.is_empty() {
        return None;
    }
    Some(Origin {
        account_id: account_id.to_string(),
        folder: folder.to_string(),
        uid: uid.parse().ok()?,
    })
}

/// Which of these came from a connected account, grouped by account.
///
/// Its own function because it is where this decides to do nothing:
/// most mail is not from a connected account, and a blob reference the
/// spool wrote must produce no note at all rather than a note nobody
/// can act on.
pub fn notes_for(blob_refs: &[String]) -> std::collections::BTreeMap<String, Vec<(String, u32)>> {
    let mut by_account: std::collections::BTreeMap<String, Vec<(String, u32)>> =
        std::collections::BTreeMap::new();
    for blob in blob_refs {
        if let Some(o) = origin_of(blob) {
            by_account
                .entry(o.account_id)
                .or_default()
                .push((o.folder, o.uid));
        }
    }
    by_account
}

/// The key of one account's pending flag notes.
pub fn pending_key(account_id: &str) -> String {
    format!("ext:writeback:{account_id}")
}

/// Leave a note that these messages were read here.
///
/// One field per message, so a second read of the same message
/// overwrites rather than queueing twice — the note says what is true
/// now, not what happened.
pub(crate) fn note_read(state: &Arc<FastcoreState>, blob_refs: &[String]) {
    let by_account = notes_for(blob_refs);
    if by_account.is_empty() {
        return;
    }
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    for (account, items) in by_account {
        let key = pending_key(&account);
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = items
            .iter()
            .map(|(folder, uid)| (format!("{folder}:{uid}").into_bytes(), b"seen".to_vec()))
            .collect();
        let refs: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(f, v)| (f.as_slice(), v.as_slice()))
            .collect();
        let _ = conn.hset(key.as_bytes(), &refs);
        // Set after the write, and reset by a later read of the same
        // message — a note that keeps being true keeps its week.
        let fields: Vec<&[u8]> = pairs.iter().map(|(f, _)| f.as_slice()).collect();
        let _ = conn.hexpire(
            key.as_bytes(),
            &fields,
            NOTE_TTL,
            kevy_client::HExpireCond::Always,
        );
    }
}

/// The notes waiting for one account, grouped by folder.
///
/// Returned grouped because IMAP is: one `SELECT` per folder, then one
/// `UID STORE` for every uid in it. Ungrouped this would select the
/// same folder once per message.
pub(crate) fn pending_for(state: &Arc<FastcoreState>, account_id: &str) -> Vec<(String, Vec<u32>)> {
    let Some(mut conn) = state.net_conn() else {
        return Vec::new();
    };
    let flat = conn
        .hgetall(pending_key(account_id).as_bytes())
        .unwrap_or_default();
    group_pending(&flat)
}

/// Group `folder:uid` field names into one entry per folder.
///
/// Its own function because the parsing is where this can go wrong and
/// a store is not needed to test it: a folder name carries no colon
/// after `sanitise`, so the uid is what follows the **last** one.
pub fn group_pending(flat: &[Vec<u8>]) -> Vec<(String, Vec<u32>)> {
    let mut by_folder: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let field = String::from_utf8_lossy(&flat[i]).to_string();
        if let Some((folder, uid)) = field.rsplit_once(':')
            && let Ok(uid) = uid.parse::<u32>()
            && !folder.is_empty()
        {
            by_folder.entry(folder.to_string()).or_default().push(uid);
        }
        i += 2;
    }
    by_folder.into_iter().collect()
}

/// How long a note may sit before it stops being worth carrying.
///
/// A queue with a permanently failing item only grows: a folder that
/// cannot be selected, or a mailbox the provider has made read-only,
/// fails identically on every pass and forever. So a note expires on
/// its own — a week is long enough to ride out a provider having a bad
/// few days, and short enough that the queue converges, which is the
/// property that matters because nobody watches this.
///
/// A TTL rather than an attempt count because the failure is about
/// time, not about tries: six retries in an hour says nothing, and the
/// store can drop these without anybody running a sweep.
pub const NOTE_TTL: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// Forget the notes that have been carried.
pub(crate) fn clear_pending(state: &Arc<FastcoreState>, account_id: &str, done: &[(String, u32)]) {
    if done.is_empty() {
        return;
    }
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let fields: Vec<Vec<u8>> = done
        .iter()
        .map(|(f, uid)| format!("{f}:{uid}").into_bytes())
        .collect();
    let refs: Vec<&[u8]> = fields.iter().map(Vec::as_slice).collect();
    let _ = conn.hdel(pending_key(account_id).as_bytes(), &refs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_from_a_connected_account_says_where_it_came_from() {
        assert_eq!(
            origin_of("ext-acc_1-INBOX-4390"),
            Some(Origin {
                account_id: "acc_1".into(),
                folder: "INBOX".into(),
                uid: 4390,
            })
        );
    }

    /// `sanitise` maps every non-alphanumeric byte to `_`, so a Gmail
    /// folder arrives full of separators. Splitting from both ends is
    /// what survives it.
    #[test]
    fn a_folder_full_of_separators_still_parses() {
        let o = origin_of("ext-ext_1754_ab-_Gmail__Sent_Mail-12").expect("parses");
        assert_eq!(o.account_id, "ext_1754_ab");
        assert_eq!(o.folder, "_Gmail__Sent_Mail");
        assert_eq!(o.uid, 12);
    }

    /// Everything else is most mail: the spool writes maildir names.
    #[test]
    fn ordinary_mail_has_no_origin() {
        assert_eq!(origin_of("1754400000.M1.golia:2,S"), None);
        // A hyphenated name that is not ours parses fine without the
        // prefix check, which is why the check is there.
        assert_eq!(origin_of("a1b2-c3d4-5"), None);
        assert_eq!(origin_of(""), None);
        assert_eq!(origin_of("ext-"), None);
        assert_eq!(origin_of("ext-acc-INBOX-notanumber"), None);
        assert_eq!(origin_of("ext-acc-INBOX"), None);
    }

    /// One `SELECT` per folder, not one per message.
    #[test]
    fn notes_are_grouped_by_folder() {
        let flat: Vec<Vec<u8>> = ["INBOX:3", "seen", "INBOX:1", "seen", "Archive:9", "seen"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        let got = group_pending(&flat);
        assert_eq!(got.len(), 2, "a folder was visited twice: {got:?}");
        let inbox = got.iter().find(|(f, _)| f == "INBOX").expect("INBOX");
        assert_eq!(inbox.1.len(), 2);
    }

    #[test]
    fn a_field_that_is_not_a_note_is_ignored_rather_than_guessed() {
        let flat: Vec<Vec<u8>> = ["nonsense", "seen", "INBOX:x", "seen", ":3", "seen"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        assert!(group_pending(&flat).is_empty());
    }

    /// Most mail is not from a connected account, and a maildir name
    /// must produce no note at all — a queue of jobs nobody can act on
    /// is worse than no queue.
    #[test]
    fn ordinary_mail_leaves_no_note() {
        let refs = vec![
            "1754400000.M1.golia:2,S".to_string(),
            // With a hyphen, because that is what makes the `ext-`
            // prefix load-bearing rather than incidental: without it
            // this name parses into an account called "a1b2".
            "a1b2-c3d4-5".to_string(),
        ];
        assert!(
            notes_for(&refs).is_empty(),
            "a maildir name produced a note"
        );
    }

    #[test]
    fn a_read_thread_leaves_one_note_per_message() {
        let refs = vec![
            "ext-acc_1-INBOX-3".to_string(),
            "ext-acc_1-INBOX-4".to_string(),
            "1754400000.M1.golia:2,S".to_string(),
        ];
        let notes = notes_for(&refs);
        assert_eq!(notes.len(), 1, "more than one account: {notes:?}");
        assert_eq!(notes["acc_1"].len(), 2, "a message was dropped");
    }

    /// A thread can hold mail from two accounts — a reply from one
    /// side, the original from another — and each server has to be
    /// told about its own.
    #[test]
    fn two_accounts_in_one_thread_are_kept_apart() {
        let refs = vec![
            "ext-acc_1-INBOX-3".to_string(),
            "ext-acc_2-Archive-99".to_string(),
        ];
        let notes = notes_for(&refs);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes["acc_2"], vec![("Archive".to_string(), 99)]);
    }

    /// A queue with a permanently failing item only grows, and this
    /// one has two such paths: a folder that cannot be selected, and a
    /// mailbox the provider has made read-only. Both fail identically
    /// on every pass and forever.
    ///
    /// The TTL is what makes it converge without anybody sweeping. A
    /// week is a judgement — long enough to ride out a provider having
    /// a bad few days — but the bound is not: a note with no expiry at
    /// all is the defect.
    #[test]
    fn a_note_does_not_wait_forever() {
        assert!(
            NOTE_TTL.as_secs() > 24 * 3600,
            "a day is not long enough to ride out a provider having a bad night"
        );
        assert!(
            NOTE_TTL.as_secs() <= 30 * 24 * 3600,
            "a note nobody could carry in a month is not going to be carried"
        );
    }
}
