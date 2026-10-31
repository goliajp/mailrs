//! The Trash for IMAP, from `trashed_conversations`.

use mailrs_mail_protocols::backend::{Error, Message, Result, Trashed, TrashedMessage};
use mailrs_mailbox::Mailbox;

use super::{PgAccount, store, to_message};

impl PgAccount {
    /// Messages keep their mailbox's uid; in `Trash` they go by row id,
    /// which no two of this user's messages share.
    pub(super) async fn trashed_view(&self) -> Result<Trashed<Mailbox, String>> {
        let (rows, latest) = self
            .mailbox
            .trashed_messages(&self.user)
            .await
            .map_err(store)?;
        let mailboxes = self.mailboxes().await?;
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(mb) = mailboxes.iter().find(|m| m.id == row.mailbox_id) else {
                continue;
            };
            let uid = u32::try_from(row.id)
                .map_err(|_| Error::Store(format!("message id {} is past a uid", row.id)))?;
            messages.push(TrashedMessage {
                mailbox: mb.clone(),
                uid,
                message: to_message(0, row),
            });
        }
        Ok(Trashed { messages, latest })
    }

    pub(super) async fn set_trashed_thread(
        &self,
        mailbox: &Mailbox,
        m: &Message<String>,
        trashed: bool,
    ) -> Result<bool> {
        let Some(row) = self
            .mailbox
            .get_message(mailbox.id, m.uid)
            .await
            .map_err(store)?
            .filter(|r| !r.thread_id.is_empty())
        else {
            return Ok(false);
        };
        match trashed {
            true => {
                let now = chrono::Utc::now().timestamp();
                self.mailbox
                    .trash_thread(&self.user, &row.thread_id, now)
                    .await
            }
            false => {
                self.mailbox
                    .untrash_thread(&self.user, &row.thread_id)
                    .await
            }
        }
        .map_err(store)?;
        Ok(true)
    }
}
