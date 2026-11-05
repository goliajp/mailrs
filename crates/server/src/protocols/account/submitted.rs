//! An APPEND to Sent of a message the user already has there: the copy
//! filed for their SMTP submission gives way to the client's own, once;
//! any other copy answers the APPEND.

use mailrs_core_sidestate::families::submitted;
use mailrs_mail_protocols::backend::{Account, Error, Filed, FiledCopy, Flag, Result};
use mailrs_mailbox::{FLAG_DELETED, Mailbox, maildir_flags_to_bitmask};

use super::{PgAccount, store};

impl PgAccount {
    fn net(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(self.net_url.as_deref()?).ok()
    }

    pub(super) async fn copy_filed(
        &self,
        mailbox: &Mailbox,
        bytes: &[u8],
    ) -> Result<Option<FiledCopy>> {
        let message_id = mailrs_intake::headers::message_id_of(bytes);
        if message_id.is_empty() {
            return Ok(None);
        }
        let uid = self
            .mailbox
            .uid_by_message_id(mailbox.id, &message_id)
            .await
            .map_err(store)?;
        let Some(uid) = uid else {
            return Ok(None);
        };
        let submitted = match self.net() {
            Some(mut c) => submitted::is_marked(&mut c, &self.user, &message_id)?,
            None => false,
        };
        Ok(Some(FiledCopy {
            filed: Filed {
                uidvalidity: mailbox.uidvalidity,
                uid,
            },
            submitted,
        }))
    }

    /// The client's copy is filed and indexed first, under a new uid, then
    /// the submission's row and body go.
    pub(super) async fn replace_filed(
        &self,
        mailbox: &Mailbox,
        old: Filed,
        bytes: &[u8],
        flags: &[Flag],
    ) -> Result<Filed> {
        let message_id = mailrs_intake::headers::message_id_of(bytes);
        let gone = self
            .messages(mailbox)
            .await?
            .into_iter()
            .find(|m| m.uid == old.uid)
            .ok_or_else(|| Error::Store(format!("the copy of {message_id} to replace is gone")))?;
        let uid = self
            .deliver(mailbox, bytes, maildir_flags_to_bitmask(flags))
            .await?;
        // an expunge takes only rows marked deleted
        self.mailbox
            .add_flags(mailbox.id, gone.uid, FLAG_DELETED)
            .await
            .map_err(store)?;
        let removed = self
            .mailbox
            .expunge_uids(mailbox.id, &[gone.uid])
            .await
            .map_err(store)?;
        self.remove_bodies(std::slice::from_ref(&gone), &removed)
            .await?;
        if let Some(mut c) = self.net() {
            submitted::clear(&mut c, &self.user, &message_id)?;
        }
        Ok(Filed {
            uidvalidity: mailbox.uidvalidity,
            uid,
        })
    }
}
