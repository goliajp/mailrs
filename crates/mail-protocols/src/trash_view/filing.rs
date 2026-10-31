//! COPY, MOVE and EXPUNGE across `Trash` and the mailboxes messages are
//! filed in.

use super::{TRASH, TrashView, ViewKey, ViewMailbox, trash_uidvalidity, unwrap};
use crate::backend::{Account, Error, Expunged, Filed, Message, Result};

impl<A: Account> TrashView<A> {
    /// Where `Trash` stands now, for the reply to a move or copy into it.
    async fn filed_in_trash(&self, mailbox: &A::Mailbox, uid: u32) -> Result<Filed> {
        let name = self.inner.mailbox_name(mailbox).to_string();
        let trashed = self.inner.trashed().await?;
        let uid = trashed
            .messages
            .iter()
            .find(|t| t.message.uid == uid && self.inner.mailbox_name(&t.mailbox) == name)
            .map(|t| t.uid)
            .ok_or_else(|| Error::Store("the message did not reach the Trash".into()))?;
        Ok(Filed {
            uidvalidity: trash_uidvalidity(trashed.latest),
            uid,
        })
    }

    /// Bring the conversation of a message in `Trash` back, and file the
    /// message in `to` when it is filed elsewhere.
    async fn out_of_trash(
        &self,
        m: &Message<ViewKey<A::Mailbox, A::Key>>,
        to: &A::Mailbox,
        keep_copy: bool,
    ) -> Result<Filed> {
        let (msg, filed) = unwrap(m);
        let from = filed.ok_or_else(|| Error::Store("not a message of Trash".into()))?;
        // `false` is a message filed in a mailbox named Trash with no
        // conversation behind it: moving the file is all there is to do
        self.inner.set_trashed(&from, &msg, false).await?;
        if self.inner.mailbox_name(&from) == self.inner.mailbox_name(to) {
            let selected = self.inner.select(to).await?;
            return Ok(Filed {
                uidvalidity: selected.uidvalidity,
                uid: msg.uid,
            });
        }
        match keep_copy {
            true => self.inner.copy(&from, &msg, to).await,
            false => self.inner.move_to(&from, &msg, to).await,
        }
    }
    /// COPY or MOVE. Into `Trash` it moves the message's conversation there
    /// and leaves the file where it is: the Trash is kept per conversation.
    /// Out of `Trash` it brings the conversation back.
    pub(super) async fn file(
        &self,
        from: &ViewMailbox<A::Mailbox>,
        message: &Message<ViewKey<A::Mailbox, A::Key>>,
        to: &ViewMailbox<A::Mailbox>,
        keep_copy: bool,
    ) -> Result<Filed> {
        match (from, to) {
            (ViewMailbox::Filed(f), ViewMailbox::Trash { .. }) => {
                let (msg, _) = unwrap(message);
                if self.inner.set_trashed(f, &msg, true).await? {
                    return self.filed_in_trash(f, msg.uid).await;
                }
                // no conversation to move: file the message itself in the
                // mailbox named Trash, which `Trash` shows whole
                let real = self.inner.mailbox(TRASH).await?.ok_or_else(|| {
                    Error::Refused("[TRYCREATE] there is no Trash mailbox to file it in".into())
                })?;
                let there = match keep_copy {
                    true => self.inner.copy(f, &msg, &real).await?,
                    false => self.inner.move_to(f, &msg, &real).await?,
                };
                self.filed_in_trash(&real, there.uid).await
            }
            (ViewMailbox::Trash { .. }, ViewMailbox::Filed(t)) => {
                self.out_of_trash(message, t, keep_copy).await
            }
            (ViewMailbox::Trash { .. }, ViewMailbox::Trash { .. }) => {
                let (msg, filed) = unwrap(message);
                let f = filed.ok_or_else(|| Error::Store("not a message of Trash".into()))?;
                self.filed_in_trash(&f, msg.uid).await
            }
            (ViewMailbox::Filed(f), ViewMailbox::Filed(t)) => {
                let (msg, _) = unwrap(message);
                match keep_copy {
                    true => self.inner.copy(f, &msg, t).await,
                    false => self.inner.move_to(f, &msg, t).await,
                }
            }
        }
    }

    /// An EXPUNGE outside `Trash`. A message that went to the Trash since
    /// the client listed the mailbox is not deleted: clients without MOVE
    /// delete by COPY to Trash, then `\Deleted` and EXPUNGE here, and the
    /// copy is the same file. It has left this mailbox all the same.
    pub(super) async fn expunge_filed(
        &self,
        mailbox: &A::Mailbox,
        doomed: &[Message<ViewKey<A::Mailbox, A::Key>>],
    ) -> Expunged {
        let hidden = match self.hidden(mailbox).await {
            Ok(h) => h,
            Err(e) => {
                return Expunged {
                    removed: Vec::new(),
                    failed: Some(e),
                };
            }
        };
        let (kept_aside, really): (Vec<_>, Vec<_>) =
            doomed.iter().partition(|m| hidden.contains(&m.uid));
        let really: Vec<_> = really.into_iter().map(|m| unwrap(m).0).collect();
        let mut out = self.inner.expunge(mailbox, &really).await;
        out.removed.extend(kept_aside.iter().map(|m| m.uid));
        out
    }

    /// An EXPUNGE in `Trash`: the messages are deleted for good, each from
    /// the mailbox it is filed in.
    pub(super) async fn expunge_trash(
        &self,
        doomed: &[Message<ViewKey<A::Mailbox, A::Key>>],
    ) -> Expunged {
        let mut out = Expunged::default();
        for m in doomed {
            let (msg, filed) = unwrap(m);
            let Some(from) = filed else { continue };
            let done = self.inner.expunge(&from, std::slice::from_ref(&msg)).await;
            if let Some(e) = done.failed {
                out.failed = Some(e);
                break;
            }
            if !done.removed.is_empty() {
                out.removed.push(m.uid);
            }
        }
        out
    }
}
