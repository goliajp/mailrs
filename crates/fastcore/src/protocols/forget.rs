//! The index follows a message file an IMAP EXPUNGE or a POP3 DELE took
//! away. Without this the file went and its row stayed: the conversation
//! went on listing a message nobody could open, as the SQL core, which
//! deletes the row, never did.

use std::path::{Path, PathBuf};

use mailrs_mailbox_kevy::Forgotten;

use super::account::FcMailbox;
use super::real::{RealAccount, base_name};
use crate::imap::backend;

/// What the index knows a file by: its Message-ID (or the id a file
/// without one is given) and its `blob_ref`. Read before the file goes.
pub(super) struct Doomed {
    message_id: String,
    blob_ref: String,
}

impl Doomed {
    pub(super) fn read(mailbox: &FcMailbox, path: &Path) -> Option<Self> {
        let base = base_name(path)?;
        let blob_ref = backend::blob_ref_for(mailbox, base);
        let bytes = std::fs::read(path).ok()?;
        let head = &bytes[..bytes.len().min(16 * 1024)];
        let (header_id, ..) = mailrs_intake::headers::extract_headers(head);
        let message_id = mailrs_intake::headers::message_id_or_file_id(header_id, &blob_ref);
        Some(Self {
            message_id,
            blob_ref,
        })
    }
}

impl RealAccount {
    /// `doomed`'s file is gone: take the message out of the index, or point
    /// it at another copy still on disk, and decide what is left of its
    /// conversation's section again.
    pub(super) fn forget(&self, doomed: &Doomed) {
        let home = self.home();
        let on_disk = |name: &str| mailrs_maildir::locate(&home, name).is_some();
        let result = self.state.mailbox.forget_message_file(
            &self.user,
            &doomed.message_id,
            &doomed.blob_ref,
            on_disk,
        );
        match result {
            Ok(Forgotten::Message { thread_id }) => {
                crate::sections::refold(&self.state, &self.user, &thread_id, &home);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(
                error = %e,
                user = %self.user,
                message_id = %doomed.message_id,
                "removed message still indexed"
            ),
        }
    }

    fn home(&self) -> PathBuf {
        let (local, domain) = self.user.split_once('@').unwrap_or((&self.user, ""));
        self.maildir_root.join(domain).join(local)
    }
}
