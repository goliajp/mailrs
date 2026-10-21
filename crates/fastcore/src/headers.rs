//! Reading a message far enough to file it. The readings both cores share
//! live in `mailrs_intake::headers`; the threading resolver and the
//! maildir filename conventions this core's sweep needs are here.
//!
//! Shared by the ingest path and the maildir sweep on purpose. A second
//! copy of `thread_root` would thread the sweep's repairs
//! differently from the arrivals they repair, and the symptom of that is
//! two messages that should be one conversation and are not.

use std::sync::Arc;

pub(crate) use mailrs_intake::headers::{
    body_text_for_search, extract_headers, extract_sender_trust, maildir_filename_epoch, sort_date,
};

use crate::FastcoreState;

/// This core's thread index, as the shared resolver asks about it.
pub(crate) struct Threads<'a>(pub &'a Arc<FastcoreState>);

impl mailrs_intake::arrival::Threads for Threads<'_> {
    fn thread_for_message_id(&self, user: &str, message_id: &str) -> Option<String> {
        self.0
            .mailbox
            .thread_for_message_id(user, message_id)
            .ok()
            .flatten()
    }

    fn thread_subject(&self, _user: &str, thread_id: &str) -> Option<String> {
        match self.0.mailbox.get_thread(thread_id) {
            Ok(Some(row)) => Some(row.subject),
            _ => None,
        }
    }
}

/// The conversation a message belongs to; see
/// `mailrs_intake::arrival::thread_root`.
pub(crate) fn thread_root(
    state: &Arc<FastcoreState>,
    user: &str,
    message_id: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> String {
    mailrs_intake::arrival::thread_root(
        &Threads(state),
        user,
        message_id,
        in_reply_to,
        references,
        subject,
    )
}

/// Whether a maildir filename carries the \Seen flag — the `:2,` info
/// section lists flags alphabetically (`...:2,RS` etc.).
pub(crate) fn maildir_seen_flag(name: &str) -> bool {
    match name.rsplit_once(":2,") {
        Some((_, info)) => info.contains('S'),
        None => false,
    }
}

/// The Maildir++ keyword bits in a file name's `:2,` suffix.
///
/// Lowercase letters, whose meaning is in the mailbox's `mailrs-keywords`
/// file — `archived` and `pinned` are written there because no standard
/// flag means either and because a person's decision cannot be recomputed
/// from the mail.
pub(crate) fn maildir_keyword_bits(name: &str) -> Vec<char> {
    match name.rsplit_once(":2,") {
        Some((_, info)) => {
            let mut out: Vec<char> = info.chars().filter(|c| c.is_ascii_lowercase()).collect();
            out.sort_unstable();
            out.dedup();
            out
        }
        None => Vec::new(),
    }
}

/// Fall back to the file's mtime as the delivery epoch when both the
/// `Date:` header and the maildir filename yield nothing usable.
pub(crate) fn file_mtime_epoch(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}
