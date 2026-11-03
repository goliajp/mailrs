//! Archive and All Mail for IMAP: the web's Archived and All Mail lists,
//! read off the same thread rows, with each message by row id as in `Trash`.

use mailrs_mail_protocols::backend::{Error, Message, Result, ShownMessage, Trashed};
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::Scope;
use mailrs_mailbox::{Mailbox, MessageMeta};

use super::{PgAccount, store, to_message};

impl PgAccount {
    /// All Mail's range, archived conversations only.
    pub(super) async fn archived_view(&self) -> Result<Trashed<Mailbox, String>> {
        self.listed_view(Some(true)).await
    }

    /// Every conversation but Junk, the Trash and held mail.
    pub(super) async fn all_mail_view(&self) -> Result<Trashed<Mailbox, String>> {
        self.listed_view(None).await
    }

    async fn listed_view(&self, archived: Option<bool>) -> Result<Trashed<Mailbox, String>> {
        let q = ThreadQuery {
            scope: Scope::AllMail,
            archived,
            trashed: Some(false),
            held: Held::Hide,
            ..ThreadQuery::all(&self.user)
        };
        let rows = self.mailbox.thread_rows(&q).await.map_err(store)?;
        let tids: Vec<String> = rows.into_iter().map(|r| r.thread_id).collect();
        let messages = self
            .mailbox
            .messages_of_threads(&self.user, &tids)
            .await
            .map_err(store)?;
        Ok(Trashed {
            messages: self.shown_rows(messages).await?,
            latest: 0,
        })
    }

    /// Each row where it is filed, by row id.
    pub(super) async fn shown_rows(
        &self,
        rows: Vec<MessageMeta>,
    ) -> Result<Vec<ShownMessage<Mailbox, String>>> {
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
        Ok(messages)
    }

    /// Archive the message's conversation the way the web does.
    pub(super) async fn set_archived_thread(
        &self,
        mailbox: &Mailbox,
        m: &Message<String>,
        archived: bool,
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
        match archived {
            true => {
                self.mailbox
                    .archive_thread(&self.user, &row.thread_id)
                    .await
            }
            false => {
                self.mailbox
                    .unarchive_thread(&self.user, &row.thread_id)
                    .await
            }
        }
        .map_err(store)?;
        Ok(true)
    }
}
