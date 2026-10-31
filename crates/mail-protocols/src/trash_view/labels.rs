//! Labels as mailboxes: each holds the messages of the conversations it is
//! on. Copying a message in puts the label on its conversation; expunging
//! or moving it out takes the label off, and no message is deleted.

use super::{TrashView, ViewKey, ViewMailbox, renumber, trash_entry};
use crate::backend::{Account, Error, Expunged, Filed, LabelFolder, Message, Result, Selected};

/// A label mailbox's UIDVALIDITY, from the uids it holds: a message joining
/// keeps its old uid, below UIDNEXT, and only a new UIDVALIDITY tells a
/// client to look again. Never 0.
pub(super) fn label_uidvalidity(uids: &[u32]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for uid in uids {
        for b in uid.to_be_bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h.max(1)
}

type Shown<A> = Vec<Message<ViewKey<<A as Account>::Mailbox, <A as Account>::Key>>>;

impl<A: Account> TrashView<A> {
    pub(super) async fn label_named(
        &self,
        name: &str,
    ) -> Result<Option<LabelFolder<A::Mailbox, A::Key>>> {
        let labels = self.inner.labels().await?;
        Ok(labels
            .into_iter()
            .find(|l| l.name.eq_ignore_ascii_case(name)))
    }

    async fn label_by_id(&self, id: &str) -> Result<LabelFolder<A::Mailbox, A::Key>> {
        let labels = self.inner.labels().await?;
        labels
            .into_iter()
            .find(|l| l.id == id)
            .ok_or_else(|| Error::Refused("[NONEXISTENT] the label is gone".into()))
    }

    pub(super) async fn label_messages(&self, id: &str) -> Result<Shown<A>> {
        let label = self.label_by_id(id).await?;
        let mut out: Vec<_> = label.messages.iter().map(trash_entry).collect();
        out.sort_by_key(|m| m.uid);
        renumber(&mut out);
        Ok(out)
    }

    pub(super) async fn label_selected(
        &self,
        id: &str,
    ) -> Result<Selected<ViewKey<A::Mailbox, A::Key>>> {
        let messages = self.label_messages(id).await?;
        let uids: Vec<u32> = messages.iter().map(|m| m.uid).collect();
        Ok(Selected {
            uidvalidity: label_uidvalidity(&uids),
            uidnext: uids.iter().max().unwrap_or(&0) + 1,
            highest_modseq: messages.iter().map(|m| m.modseq).max().unwrap_or(0),
            recent: messages.len() as u32,
            messages,
        })
    }

    /// Where label `id` stands now, for the reply to a copy or move into it.
    pub(super) async fn filed_in_label(
        &self,
        id: &str,
        mailbox: &A::Mailbox,
        uid: u32,
    ) -> Result<Filed> {
        let name = self.inner.mailbox_name(mailbox).to_string();
        let label = self.label_by_id(id).await?;
        let mut uids: Vec<u32> = label.messages.iter().map(|t| t.uid).collect();
        uids.sort_unstable();
        let shown = label
            .messages
            .iter()
            .find(|t| t.message.uid == uid && self.inner.mailbox_name(&t.mailbox) == name)
            .map(|t| t.uid)
            .ok_or_else(|| Error::Store("the message did not reach the label".into()))?;
        Ok(Filed {
            uidvalidity: label_uidvalidity(&uids),
            uid: shown,
        })
    }

    /// Put label `id` on the message's conversation, refusing a message that
    /// belongs to none.
    pub(super) async fn put_label(
        &self,
        mailbox: &A::Mailbox,
        msg: &Message<A::Key>,
        id: &str,
        on: bool,
    ) -> Result<()> {
        if self.inner.set_label(mailbox, msg, id, on).await? {
            return Ok(());
        }
        Err(Error::Refused(
            "[CANNOT] only a message of a conversation can carry a label".into(),
        ))
    }

    /// An EXPUNGE in a label: the label comes off; no message is deleted.
    pub(super) async fn expunge_label(
        &self,
        id: &str,
        doomed: &[Message<ViewKey<A::Mailbox, A::Key>>],
    ) -> Expunged {
        let mut out = Expunged::default();
        for m in doomed {
            let (msg, filed) = super::unwrap(m);
            let Some(from) = filed else { continue };
            if let Err(e) = self.put_label(&from, &msg, id, false).await {
                out.failed = Some(e);
                break;
            }
            out.removed.push(m.uid);
        }
        out
    }

    /// The label's view of the mailboxes: `name` for a label, unless it is
    /// the name of one already filed (a label takes the name over).
    pub(super) async fn label_mailbox(
        &self,
        name: &str,
    ) -> Result<Option<ViewMailbox<A::Mailbox>>> {
        if self.inner.read_only().is_some() {
            return Ok(None);
        }
        let Some(l) = self.label_named(name).await? else {
            return Ok(None);
        };
        Ok(Some(ViewMailbox::Label {
            id: l.id,
            name: l.name,
            inbox: self.inbox().await?,
        }))
    }

    /// Out of a label into a mailbox: MOVE takes the label off, COPY leaves
    /// it; the message is filed in `to` when it is filed elsewhere.
    pub(super) async fn out_of_label(
        &self,
        id: &str,
        m: &Message<ViewKey<A::Mailbox, A::Key>>,
        to: &A::Mailbox,
        keep_copy: bool,
    ) -> Result<Filed> {
        let (msg, filed) = super::unwrap(m);
        let from = filed.ok_or_else(|| Error::Store("not a message shown here".into()))?;
        if !keep_copy {
            self.put_label(&from, &msg, id, false).await?;
        }
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
}
