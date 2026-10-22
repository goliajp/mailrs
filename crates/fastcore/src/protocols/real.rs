//! A real account: mailboxes are the user's Maildir++ folders, UIDs come
//! from the per-user allocator the web API shares, modseqs and the
//! vanished log live in the embedded kevy.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_mail_protocols::backend::{Expunged, Filed, Flag, Message, Result, Selected};

use super::account::{FcMailbox, to_imap, to_message};
use crate::FastcoreState;
use crate::imap::backend;

pub struct RealAccount {
    pub(crate) state: Arc<FastcoreState>,
    pub(crate) user: String,
    pub(crate) maildir_root: PathBuf,
}

/// The base name of a Maildir file, without its `:2,` info.
fn base_name(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(|f| f.to_str())
        .and_then(|f| f.split(':').next())
}

impl RealAccount {
    pub(crate) fn mailbox_names(&self) -> Vec<String> {
        backend::list_mailboxes_in(&self.maildir_root, &self.user)
            .into_iter()
            .map(|m| m.name)
            .collect()
    }

    pub(crate) fn mailbox(&self, name: &str) -> Option<FcMailbox> {
        backend::get_mailbox_in(&self.maildir_root, &self.user, name)
    }

    pub(crate) fn messages(&self, mailbox: &FcMailbox) -> Vec<Message<PathBuf>> {
        backend::list_messages(&self.state, &self.user, mailbox)
            .into_iter()
            .map(to_message)
            .collect()
    }

    pub(crate) fn select(&self, mailbox: &FcMailbox) -> Selected<PathBuf> {
        let messages = self.messages(mailbox);
        Selected {
            uidvalidity: backend::uidvalidity(&self.state, &self.user, &mailbox.name),
            uidnext: backend::uid_next(&self.state, &self.user),
            highest_modseq: backend::highest_modseq(&self.state, &self.user),
            // every scan is fresh; no message has been seen by a session
            recent: messages.len() as u32,
            messages,
        }
    }

    /// The file name first, then the modseq, then the index — in that
    /// order, since the file is the authority.
    pub(crate) fn store_flags(&self, m: &Message<PathBuf>, flags: &[Flag]) -> Result<()> {
        let msg = to_imap(m);
        backend::set_flags(&msg, flags)?;
        let modseq = backend::bump_modseq(&self.state, &self.user);
        if let Some(base) = base_name(&msg.path) {
            backend::set_file_modseq(&self.state, &self.user, base, modseq);
        }
        sync_index_after_store(&self.state, &self.user, msg.uid, flags);
        Ok(())
    }

    fn filed(&self, to: &FcMailbox, uid: u32) -> Filed {
        Filed {
            uidvalidity: backend::uidvalidity(&self.state, &self.user, &to.name),
            uid,
        }
    }

    pub(crate) fn copy(&self, m: &Message<PathBuf>, to: &FcMailbox) -> Result<Filed> {
        let uid = backend::copy_to(&self.state, &self.user, &to_imap(m), to)?;
        Ok(self.filed(to, uid))
    }

    /// The source copy goes as an EXPUNGE takes it: logged as vanished,
    /// so a QRESYNC client that was not connected learns of it.
    pub(crate) fn move_to(
        &self,
        from: &FcMailbox,
        m: &Message<PathBuf>,
        to: &FcMailbox,
    ) -> Result<Filed> {
        let msg = to_imap(m);
        let uid = backend::move_to(&self.state, &self.user, &msg, to)?;
        let ms = backend::bump_modseq(&self.state, &self.user);
        backend::record_vanished(&self.state, &self.user, &from.name, m.uid, ms);
        Ok(self.filed(to, uid))
    }

    pub(crate) fn expunge(&self, mailbox: &FcMailbox, doomed: &[Message<PathBuf>]) -> Expunged {
        let mut out = Expunged::default();
        for m in doomed {
            match backend::remove_message(&self.user, &to_imap(m)) {
                Ok(()) => {
                    let ms = backend::bump_modseq(&self.state, &self.user);
                    backend::record_vanished(&self.state, &self.user, &mailbox.name, m.uid, ms);
                }
                // removed by someone else in the meantime: gone all the same
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    out.failed = Some(e.into());
                    break;
                }
            }
            out.removed.push(m.uid);
        }
        out
    }

    /// The flags reach the file name and, as a STORE carries them, the
    /// index.
    pub(crate) fn append(
        &self,
        mailbox: &FcMailbox,
        bytes: &[u8],
        flags: &[Flag],
    ) -> Result<Filed> {
        let (uid, _) = backend::append(&self.state, &self.user, mailbox, bytes, flags)?;
        if !flags.is_empty() {
            sync_index_after_store(&self.state, &self.user, uid, flags);
        }
        Ok(self.filed(mailbox, uid))
    }

    pub(crate) fn vanished_since(&self, mailbox: &FcMailbox, modseq: u64) -> Vec<u32> {
        backend::vanished_since(&self.state, &self.user, &mailbox.name, modseq)
    }
}

/// Carry an IMAP `STORE` through to the index, using the same code the
/// core-api flags route runs.
///
/// Without it a message read in an IMAP client stayed unread in the
/// conversation list and the badge, and the engagement signal the ranker
/// learns from was never recorded. Every failure is logged and left: the
/// file name — the authoritative copy — is already written, and an index
/// behind it is the direction a reindex repairs. Failing the command
/// would tell the client the store did not happen when the file says it
/// did.
fn sync_index_after_store(state: &Arc<FastcoreState>, user: &str, uid: u32, merged: &[Flag]) {
    use mailrs_core_api::method::message::{MessageWire, maildir_flags_to_bitmask};

    let Ok(Some(bytes)) = state.mailbox.get_message_by_uid(user, uid) else {
        return;
    };
    let Ok(mut wire) = serde_json::from_slice::<MessageWire>(&bytes) else {
        tracing::warn!(%user, uid, "STORE: wire parse failed; index left behind");
        return;
    };
    let old_flags = wire.flags;
    wire.flags = maildir_flags_to_bitmask(merged);
    if wire.flags == old_flags {
        return;
    }
    let Ok(json) = serde_json::to_vec(&wire) else {
        return;
    };
    if let Err(e) =
        crate::routes::message_ops::sync_index_to_flags(state, user, &wire, &json, old_flags)
    {
        tracing::warn!(err = %e, %user, uid, "STORE: index sync failed");
    }
}
