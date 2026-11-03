//! Archive and All Mail as IMAP sees them.
//!
//! The web archives a conversation without moving a file. A client is shown
//! that the way Gmail shows it: an archived conversation's messages leave
//! INBOX and appear in `Archive`; moving a message into `Archive` archives
//! its conversation, and moving it back to INBOX brings it back; deleting
//! one in `Archive` moves its conversation to the Trash. `All Mail` holds
//! the web's All Mail (every conversation but Junk, the Trash and held
//! mail) and is read-only: flags change there, nothing is filed in or out.
//! A core says which messages are archived and in All Mail
//! ([`Account::archived`], [`Account::all_mail`]) and archives
//! conversations ([`Account::set_archived`]).

use std::collections::HashSet;

use super::labels::label_uidvalidity;
use super::{TrashView, ViewKey, ViewMailbox, renumber, trash_entry, unwrap};
use crate::backend::{Account, Error, Expunged, Filed, Message, Result, Selected, Trashed};

pub(super) const ARCHIVE: &str = "Archive";
pub(super) const ALL_MAIL: &str = "All Mail";
const INBOX: &str = "INBOX";

type Shown<A> = Vec<Message<ViewKey<<A as Account>::Mailbox, <A as Account>::Key>>>;

pub(super) fn all_mail_is_read_only<T>() -> Result<T> {
    Err(Error::Refused(
        "[CANNOT] All Mail shows every message where it is filed; nothing moves in or out of it"
            .into(),
    ))
}

fn listed<M: Clone, K: Clone>(shown: Trashed<M, K>) -> Vec<Message<ViewKey<M, K>>> {
    let mut out: Vec<_> = shown.messages.iter().map(trash_entry).collect();
    out.sort_by_key(|m| m.uid);
    renumber(&mut out);
    out
}

/// SELECT on `Archive` or `All Mail`: the UIDVALIDITY follows the uids
/// held, so a message joining with its old uid tells a client to look again.
pub(super) fn selected<K>(messages: Vec<Message<K>>) -> Selected<K> {
    let uids: Vec<u32> = messages.iter().map(|m| m.uid).collect();
    Selected {
        uidvalidity: label_uidvalidity(&uids),
        uidnext: uids.iter().max().unwrap_or(&0) + 1,
        highest_modseq: messages.iter().map(|m| m.modseq).max().unwrap_or(0),
        recent: messages.len() as u32,
        messages,
    }
}

impl<A: Account> TrashView<A> {
    fn is_inbox(&self, mailbox: &A::Mailbox) -> bool {
        self.inner.mailbox_name(mailbox).eq_ignore_ascii_case(INBOX)
    }

    /// The messages of INBOX whose conversation is archived, by uid: they
    /// leave INBOX. Every other mailbox keeps them.
    pub(super) async fn archived_in(&self, mailbox: &A::Mailbox) -> Result<HashSet<u32>> {
        if !self.is_inbox(mailbox) {
            return Ok(HashSet::new());
        }
        let archived = self.inner.archived().await?;
        Ok(archived
            .messages
            .iter()
            .filter(|t| self.is_inbox(&t.mailbox))
            .map(|t| t.message.uid)
            .collect())
    }

    pub(super) async fn archive_messages(&self) -> Result<Shown<A>> {
        Ok(listed(self.inner.archived().await?))
    }

    pub(super) async fn all_mail_messages(&self) -> Result<Shown<A>> {
        Ok(listed(self.inner.all_mail().await?))
    }

    /// `Archive` or `All Mail` for `name`, unless a mailbox of that name is
    /// filed already: a real folder keeps its name.
    pub(super) async fn archive_mailbox(
        &self,
        name: &str,
    ) -> Result<Option<ViewMailbox<A::Mailbox>>> {
        if self.inner.read_only().is_some() || self.inner.mailbox(name).await?.is_some() {
            return Ok(None);
        }
        if name.eq_ignore_ascii_case(ARCHIVE) {
            return Ok(Some(ViewMailbox::Archive {
                inbox: self.inbox().await?,
            }));
        }
        if name.eq_ignore_ascii_case(ALL_MAIL) {
            return Ok(Some(ViewMailbox::AllMail {
                inbox: self.inbox().await?,
            }));
        }
        Ok(None)
    }

    /// LIST gains `Archive` and `All Mail` where no folder has the name.
    pub(super) fn add_archive_names(names: &mut Vec<String>) {
        for name in [ARCHIVE, ALL_MAIL] {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                names.push(name.into());
            }
        }
    }

    /// Archive the conversation of a message, or bring it back, refusing a
    /// message that belongs to none.
    pub(super) async fn put_archived(
        &self,
        mailbox: &A::Mailbox,
        msg: &Message<A::Key>,
        on: bool,
    ) -> Result<()> {
        if self.inner.set_archived(mailbox, msg, on).await? {
            return Ok(());
        }
        Err(Error::Refused(
            "[CANNOT] only a message of a conversation can be archived".into(),
        ))
    }

    /// A message filed in INBOX is back in the Inbox: its conversation is
    /// no longer archived. A message of no conversation was never hidden.
    pub(super) async fn back_in_inbox(
        &self,
        from: &A::Mailbox,
        msg: &Message<A::Key>,
        to: &A::Mailbox,
    ) -> Result<()> {
        if self.is_inbox(to) {
            self.inner.set_archived(from, msg, false).await?;
        }
        Ok(())
    }

    /// Where `Archive` stands now, for the reply to a copy or move into it.
    pub(super) async fn filed_in_archive(&self, mailbox: &A::Mailbox, uid: u32) -> Result<Filed> {
        let name = self.inner.mailbox_name(mailbox).to_string();
        let archived = self.inner.archived().await?;
        let mut uids: Vec<u32> = archived.messages.iter().map(|t| t.uid).collect();
        uids.sort_unstable();
        let shown = archived
            .messages
            .iter()
            .find(|t| t.message.uid == uid && self.inner.mailbox_name(&t.mailbox) == name)
            .map(|t| t.uid)
            .ok_or_else(|| Error::Store("the message did not reach Archive".into()))?;
        Ok(Filed {
            uidvalidity: label_uidvalidity(&uids),
            uid: shown,
        })
    }

    /// Out of `Archive` into a mailbox: into INBOX the conversation comes
    /// back to the Inbox, copied or moved; the file follows when it is filed
    /// elsewhere.
    pub(super) async fn out_of_archive(
        &self,
        m: &Message<ViewKey<A::Mailbox, A::Key>>,
        to: &A::Mailbox,
        keep_copy: bool,
    ) -> Result<Filed> {
        let (msg, filed) = unwrap(m);
        let from = filed.ok_or_else(|| Error::Store("not a message of Archive".into()))?;
        self.back_in_inbox(&from, &msg, to).await?;
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

    /// An EXPUNGE in `Archive`: deleting there is deleting, so each
    /// message's conversation goes to the Trash, as a COPY to Trash and an
    /// EXPUNGE in INBOX would have left it.
    pub(super) async fn expunge_archive(
        &self,
        doomed: &[Message<ViewKey<A::Mailbox, A::Key>>],
    ) -> Expunged {
        let mut out = Expunged::default();
        for m in doomed {
            let (msg, filed) = unwrap(m);
            let Some(from) = filed else { continue };
            match self.inner.set_trashed(&from, &msg, true).await {
                Ok(_) => out.removed.push(m.uid),
                Err(e) => {
                    out.failed = Some(e);
                    break;
                }
            }
        }
        out
    }
}
