//! The Trash and labels for IMAP, from `trashed_conversations` and
//! `thread_labels`.

use mailrs_mail_protocols::backend::{Error, LabelFolder, Message, Result, ShownMessage, Trashed};
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
            messages.push(ShownMessage {
                mailbox: mb.clone(),
                uid,
                message: to_message(0, row),
            });
        }
        Ok(Trashed { messages, latest })
    }

    /// Each label with its messages, by row id as in `Trash`.
    pub(super) async fn labels_view(&self) -> Result<Vec<LabelFolder<Mailbox, String>>> {
        let mailboxes = self.mailboxes().await?;
        let mut out = Vec::new();
        for label in self.mailbox.labels(&self.user).await.map_err(store)? {
            let rows = self
                .mailbox
                .labelled_messages(&self.user, &label.id)
                .await
                .map_err(store)?;
            let mut messages = Vec::with_capacity(rows.len());
            for row in rows {
                let Some(mb) = mailboxes.iter().find(|m| m.id == row.mailbox_id) else {
                    continue;
                };
                let uid = u32::try_from(row.id)
                    .map_err(|_| Error::Store(format!("message id {} is past a uid", row.id)))?;
                messages.push(ShownMessage {
                    mailbox: mb.clone(),
                    uid,
                    message: to_message(0, row),
                });
            }
            out.push(LabelFolder {
                id: label.id,
                name: label.name,
                messages,
            });
        }
        Ok(out)
    }

    pub(super) async fn set_label_thread(
        &self,
        mailbox: &Mailbox,
        m: &Message<String>,
        id: &str,
        on: bool,
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
        self.mailbox
            .set_label(&self.user, id, &row.thread_id, on)
            .await
            .map_err(store)?;
        Ok(true)
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
