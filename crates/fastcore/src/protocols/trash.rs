//! The Trash for IMAP: which messages belong to conversations in it, and
//! moving a message's conversation in and out.

use std::collections::HashSet;
use std::path::PathBuf;

use mailrs_mail_protocols::backend::{Error, Message, Result, Trashed, TrashedMessage};

use super::account::FcMailbox;
use super::real::{RealAccount, base_name};
use crate::imap::backend;

/// The mailbox a `blob_ref` is filed in: `.Folder/name` or a bare name for
/// INBOX.
fn folder_of(blob_ref: &str) -> &str {
    match blob_ref.strip_prefix('.').and_then(|r| r.split_once('/')) {
        Some((folder, _)) => folder,
        None => "INBOX",
    }
}

impl RealAccount {
    /// Read from the files of each conversation in the Trash, so only the
    /// mailboxes holding one are listed; with nothing in the Trash, nothing
    /// is. A mailbox named Trash is shown whole. Uids are the user's own
    /// allocator's, unique across mailboxes already.
    pub(crate) fn trashed(&self) -> Result<Trashed<FcMailbox, PathBuf>> {
        let store = &self.state.mailbox;
        let mut files: HashSet<String> = HashSet::new();
        let mut latest = 0i64;
        for tid in store.trashed_threads(&self.user).map_err(Error::Io)? {
            if let Some(row) = store
                .get_thread_for_user(&self.user, &tid)
                .map_err(Error::Io)?
            {
                latest = latest.max(row.trashed_at);
            }
            for (_, _, blob_ref) in store
                .thread_message_files(&self.user, &tid)
                .map_err(Error::Io)?
            {
                files.extend(blob_ref);
            }
        }
        let mut folders: Vec<&str> = files.iter().map(|b| folder_of(b)).collect();
        folders.push("Trash");
        folders.sort_unstable();
        folders.dedup();
        let mut messages = Vec::new();
        for name in folders {
            let Some(mb) = self.mailbox(name) else {
                continue;
            };
            let whole = name.eq_ignore_ascii_case("Trash");
            for m in self.messages(&mb) {
                let filed = base_name(&m.key).map(|b| backend::blob_ref_for(&mb, b));
                if whole || filed.is_some_and(|f| files.contains(&f)) {
                    messages.push(TrashedMessage {
                        mailbox: mb.clone(),
                        uid: m.uid,
                        message: m,
                    });
                }
            }
        }
        Ok(Trashed { messages, latest })
    }

    pub(crate) fn set_trashed(&self, m: &Message<PathBuf>, trashed: bool) -> Result<bool> {
        let Some(tid) = self.conversation_of(m)? else {
            return Ok(false);
        };
        let now = chrono::Utc::now().timestamp();
        self.state
            .mailbox
            .set_trashed(&self.user, &tid, trashed, now)
            .map_err(Error::Io)?;
        Ok(true)
    }

    /// The conversation a message file belongs to: by its uid, and when the
    /// uid index cannot answer (rows indexed with uid 0, which production
    /// has), by the message id the file gives, the way delivery derived it.
    fn conversation_of(&self, m: &Message<PathBuf>) -> Result<Option<String>> {
        let store = &self.state.mailbox;
        let thread_of = |bytes: Option<Vec<u8>>| {
            bytes
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|w| w["thread_id"].as_str().map(String::from))
                .filter(|t| !t.is_empty())
        };
        let by_uid = store
            .get_message_by_uid(&self.user, m.uid)
            .map_err(Error::Io)?;
        if let Some(tid) = thread_of(by_uid) {
            return Ok(Some(tid));
        }
        let Ok(bytes) = std::fs::read(&m.key) else {
            return Ok(None);
        };
        let head = &bytes[..bytes.len().min(16 * 1024)];
        let (header_id, ..) = mailrs_intake::headers::extract_headers(head);
        let name = m.key.file_name().and_then(|f| f.to_str()).unwrap_or("");
        let mid = mailrs_intake::headers::message_id_or_file_id(header_id, name);
        let view = store
            .user_message_view(&self.user, &mid)
            .map_err(Error::Io)?;
        Ok(thread_of(view))
    }
}
