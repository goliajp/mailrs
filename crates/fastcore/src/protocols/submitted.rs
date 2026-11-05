//! An APPEND to Sent of a message the user already has there: the copy
//! filed for their SMTP submission gives way to the client's own, once;
//! any other copy answers the APPEND.

use std::path::PathBuf;

use mailrs_core_sidestate::families::submitted;
use mailrs_mail_protocols::backend::{Error, Filed, FiledCopy, Flag, Message, Result};

use super::account::{FcMailbox, to_imap};
use super::forget::Doomed;
use super::real::{RealAccount, base_name};
use crate::imap::backend;

impl RealAccount {
    /// The message in `mailbox` that is one of the user's recorded copies
    /// of the Message-ID of `bytes`.
    fn copy_in(&self, mailbox: &FcMailbox, message_id: &str) -> Result<Option<Message<PathBuf>>> {
        let files = self.state.mailbox.message_files(&self.user, message_id)?;
        if files.is_empty() {
            return Ok(None);
        }
        Ok(self.messages(mailbox).into_iter().find(|m| {
            base_name(&m.key).is_some_and(|b| files.contains(&backend::blob_ref_for(mailbox, b)))
        }))
    }

    pub(crate) fn filed_copy(
        &self,
        mailbox: &FcMailbox,
        bytes: &[u8],
    ) -> Result<Option<FiledCopy>> {
        let message_id = mailrs_intake::headers::message_id_of(bytes);
        if message_id.is_empty() {
            return Ok(None);
        }
        let Some(m) = self.copy_in(mailbox, &message_id)? else {
            return Ok(None);
        };
        let submitted = match self.state.net_conn() {
            Some(mut c) => submitted::is_marked(&mut c, &self.user, &message_id)?,
            None => false,
        };
        Ok(Some(FiledCopy {
            filed: self.filed(mailbox, m.uid),
            submitted,
        }))
    }

    /// The client's copy is filed and indexed first, then the submission's
    /// file goes. Uids follow the Message-ID here, so the client's copy
    /// usually has the old one's uid, which then did not vanish.
    pub(crate) fn replace_submitted(
        &self,
        mailbox: &FcMailbox,
        old: Filed,
        bytes: &[u8],
        flags: &[Flag],
    ) -> Result<Filed> {
        let message_id = mailrs_intake::headers::message_id_of(bytes);
        let gone = self
            .copy_in(mailbox, &message_id)?
            .filter(|m| m.uid == old.uid)
            .ok_or_else(|| Error::Store(format!("the copy of {message_id} to replace is gone")))?;
        let doomed = Doomed::read(mailbox, &gone.key);
        let filed = self.append(mailbox, bytes, flags)?;
        backend::remove_message(&self.state, &self.user, &to_imap(&gone))?;
        let modseq = backend::bump_modseq(&self.state, &self.user);
        if filed.uid != gone.uid {
            backend::record_vanished(&self.state, &self.user, &mailbox.name, gone.uid, modseq);
        }
        if let Some(d) = &doomed {
            self.forget(d);
        }
        if let Some(mut c) = self.state.net_conn() {
            submitted::clear(&mut c, &self.user, &message_id)?;
        }
        Ok(filed)
    }
}
