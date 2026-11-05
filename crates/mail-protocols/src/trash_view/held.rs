//! Conversations held for review, as IMAP and POP3 see them.
//!
//! A fraud verdict holds a whole conversation without moving a file, and the
//! web shows it only on the review screen. Without this view a mail client
//! kept listing it in INBOX, so a held message was one click from being read
//! anywhere but the web. A held conversation's messages leave every mailbox
//! and appear in `Review`, read-only: releasing is a decision made on the
//! review screen, and a client cannot hold a message by copying it there.
//! A core only says which messages are held ([`Account::held`]).

use std::collections::HashSet;

use super::{ViewKey, renumber, trash_entry, trash_uidvalidity, unwrap, wrap};
use crate::backend::{
    Account, Error, Expunged, Filed, FiledCopy, Flag, LabelFolder, Message, Quota, Result,
    Selected, ShownMessage, Trashed,
};

/// The name the held conversations go by.
const REVIEW: &str = "Review";

/// A signed-in account, with `Review` made of its held conversations.
pub struct HeldView<A> {
    inner: A,
}

impl<A> HeldView<A> {
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

/// A mailbox of the view.
#[derive(Clone)]
pub enum HeldMailbox<M> {
    Filed(M),
    /// `inbox` is what IDLE in `Review` watches.
    Review {
        inbox: M,
    },
}

fn is_review(name: &str) -> bool {
    name.eq_ignore_ascii_case(REVIEW)
}

fn refused<T>() -> Result<T> {
    Err(Error::Refused(
        "[CANNOT] held mail is released on the review screen, not moved".into(),
    ))
}

fn shown<M, K>(held: Trashed<M, K>) -> Trashed<HeldMailbox<M>, ViewKey<HeldMailbox<M>, K>> {
    Trashed {
        messages: held
            .messages
            .into_iter()
            .map(|t| ShownMessage {
                mailbox: HeldMailbox::Filed(t.mailbox),
                uid: t.uid,
                message: wrap(t.message),
            })
            .collect(),
        latest: held.latest,
    }
}

impl<A: Account> HeldView<A> {
    /// Which messages of `mailbox` are held, by uid.
    async fn hidden(&self, mailbox: &A::Mailbox) -> Result<HashSet<u32>> {
        let name = self.inner.mailbox_name(mailbox).to_string();
        Ok(self
            .inner
            .held()
            .await?
            .messages
            .iter()
            .filter(|t| self.inner.mailbox_name(&t.mailbox) == name)
            .map(|t| t.message.uid)
            .collect())
    }

    async fn filed_messages(
        &self,
        mailbox: &A::Mailbox,
    ) -> Result<Vec<Message<ViewKey<HeldMailbox<A::Mailbox>, A::Key>>>> {
        let hidden = self.hidden(mailbox).await?;
        let mut kept: Vec<_> = self
            .inner
            .messages(mailbox)
            .await?
            .into_iter()
            .filter(|m| !hidden.contains(&m.uid))
            .map(wrap)
            .collect();
        renumber(&mut kept);
        Ok(kept)
    }

    async fn review_messages(
        &self,
    ) -> Result<(i64, Vec<Message<ViewKey<HeldMailbox<A::Mailbox>, A::Key>>>)> {
        let held = self.inner.held().await?;
        let mut out: Vec<_> = held
            .messages
            .iter()
            .map(|t| {
                let entry = trash_entry(t);
                Message {
                    key: ViewKey {
                        key: entry.key.key,
                        filed: Some((HeldMailbox::Filed(t.mailbox.clone()), t.message.uid)),
                    },
                    uid: entry.uid,
                    seqno: entry.seqno,
                    flags: entry.flags,
                    internal_date: entry.internal_date,
                    size: entry.size,
                    modseq: entry.modseq,
                }
            })
            .collect();
        out.sort_by_key(|m| m.uid);
        renumber(&mut out);
        Ok((held.latest, out))
    }

    async fn inbox(&self) -> Result<A::Mailbox> {
        self.inner
            .mailbox("INBOX")
            .await?
            .ok_or_else(|| Error::Store("no INBOX".into()))
    }

    /// The store's mailbox a message is filed in: where `Review` says it
    /// is, or the mailbox it was listed in.
    fn filed_in<'a>(
        mailbox: &'a HeldMailbox<A::Mailbox>,
        filed: &'a Option<HeldMailbox<A::Mailbox>>,
    ) -> Option<&'a A::Mailbox> {
        match (filed, mailbox) {
            (Some(HeldMailbox::Filed(m)), _) => Some(m),
            (None, HeldMailbox::Filed(m)) => Some(m),
            _ => None,
        }
    }
}

impl<A: Account> Account for HeldView<A> {
    type Mailbox = HeldMailbox<A::Mailbox>;
    type Key = ViewKey<HeldMailbox<A::Mailbox>, A::Key>;
    type Watch = A::Watch;

    fn user(&self) -> &str {
        self.inner.user()
    }

    fn read_only(&self) -> Option<&str> {
        self.inner.read_only()
    }

    async fn mailbox_names(&self) -> Result<Vec<String>> {
        let mut names = self.inner.mailbox_names().await?;
        if self.inner.read_only().is_none() && !names.iter().any(|n| is_review(n)) {
            names.push(REVIEW.into());
        }
        Ok(names)
    }

    async fn mailbox(&self, name: &str) -> Result<Option<Self::Mailbox>> {
        if is_review(name) && self.inner.read_only().is_none() {
            return Ok(Some(HeldMailbox::Review {
                inbox: self.inbox().await?,
            }));
        }
        Ok(self.inner.mailbox(name).await?.map(HeldMailbox::Filed))
    }

    fn mailbox_name<'a>(&self, mailbox: &'a Self::Mailbox) -> &'a str {
        match mailbox {
            HeldMailbox::Filed(m) => self.inner.mailbox_name(m),
            HeldMailbox::Review { .. } => REVIEW,
        }
    }

    async fn select(&self, mailbox: &Self::Mailbox) -> Result<Selected<Self::Key>> {
        match mailbox {
            HeldMailbox::Filed(m) => {
                let selected = self.inner.select(m).await?;
                let messages = self.filed_messages(m).await?;
                Ok(Selected {
                    uidvalidity: selected.uidvalidity,
                    uidnext: selected.uidnext,
                    highest_modseq: selected.highest_modseq,
                    recent: messages.len() as u32,
                    messages,
                })
            }
            HeldMailbox::Review { .. } => {
                let (latest, messages) = self.review_messages().await?;
                Ok(Selected {
                    uidvalidity: trash_uidvalidity(latest),
                    uidnext: messages.iter().map(|m| m.uid).max().unwrap_or(0) + 1,
                    highest_modseq: messages.iter().map(|m| m.modseq).max().unwrap_or(0),
                    recent: messages.len() as u32,
                    messages,
                })
            }
        }
    }

    async fn messages(&self, mailbox: &Self::Mailbox) -> Result<Vec<Message<Self::Key>>> {
        match mailbox {
            HeldMailbox::Filed(m) => self.filed_messages(m).await,
            HeldMailbox::Review { .. } => Ok(self.review_messages().await?.1),
        }
    }

    async fn read(&self, message: &Message<Self::Key>) -> Result<Option<Vec<u8>>> {
        self.inner.read(&unwrap(message).0).await
    }

    async fn store_flags(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        flags: &[Flag],
    ) -> Result<()> {
        let (msg, filed) = unwrap(message);
        match Self::filed_in(mailbox, &filed) {
            Some(m) => self.inner.store_flags(m, &msg, flags).await,
            None => Err(Error::Store("not a message shown here".into())),
        }
    }

    async fn copy(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> Result<Filed> {
        match (from, to) {
            (HeldMailbox::Filed(f), HeldMailbox::Filed(t)) => {
                self.inner.copy(f, &unwrap(message).0, t).await
            }
            _ => refused(),
        }
    }

    async fn move_to(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> Result<Filed> {
        match (from, to) {
            (HeldMailbox::Filed(f), HeldMailbox::Filed(t)) => {
                self.inner.move_to(f, &unwrap(message).0, t).await
            }
            _ => refused(),
        }
    }

    async fn expunge(&self, mailbox: &Self::Mailbox, doomed: &[Message<Self::Key>]) -> Expunged {
        match mailbox {
            HeldMailbox::Filed(m) => {
                let inner: Vec<_> = doomed.iter().map(|d| unwrap(d).0).collect();
                self.inner.expunge(m, &inner).await
            }
            // held mail is not deleted from a mail client
            HeldMailbox::Review { .. } => Expunged {
                removed: Vec::new(),
                failed: refused::<()>().err(),
            },
        }
    }

    fn unique_id(&self, message: &Message<Self::Key>) -> String {
        self.inner.unique_id(&unwrap(message).0)
    }

    fn authserv_id(&self) -> Option<&str> {
        self.inner.authserv_id()
    }

    async fn remove(&self, mailbox: &Self::Mailbox, message: &Message<Self::Key>) -> Result<()> {
        match mailbox {
            HeldMailbox::Filed(m) => self.inner.remove(m, &unwrap(message).0).await,
            HeldMailbox::Review { .. } => refused(),
        }
    }

    async fn filed_copy(&self, mailbox: &Self::Mailbox, bytes: &[u8]) -> Result<Option<FiledCopy>> {
        match mailbox {
            HeldMailbox::Filed(m) => self.inner.filed_copy(m, bytes).await,
            HeldMailbox::Review { .. } => Ok(None),
        }
    }

    async fn replace_submitted(
        &self,
        mailbox: &Self::Mailbox,
        old: Filed,
        bytes: Vec<u8>,
        flags: Vec<Flag>,
    ) -> Result<Filed> {
        match mailbox {
            HeldMailbox::Filed(m) => self.inner.replace_submitted(m, old, bytes, flags).await,
            HeldMailbox::Review { .. } => refused(),
        }
    }

    async fn append(
        &self,
        mailbox: &Self::Mailbox,
        bytes: Vec<u8>,
        flags: Vec<Flag>,
    ) -> Result<Filed> {
        match mailbox {
            HeldMailbox::Filed(m) => self.inner.append(m, bytes, flags).await,
            HeldMailbox::Review { .. } => refused(),
        }
    }

    async fn vanished_since(&self, mailbox: &Self::Mailbox, modseq: u64) -> Result<Vec<u32>> {
        match mailbox {
            // what was held left this mailbox too; a client drops uids it
            // never had
            HeldMailbox::Filed(m) => {
                let mut gone = self.inner.vanished_since(m, modseq).await?;
                gone.extend(self.hidden(m).await?);
                gone.sort_unstable();
                gone.dedup();
                Ok(gone)
            }
            HeldMailbox::Review { .. } => Ok(Vec::new()),
        }
    }

    async fn quota(&self) -> Result<Quota> {
        self.inner.quota().await
    }

    fn watch(&self, mailbox: &Self::Mailbox) -> Self::Watch {
        match mailbox {
            HeldMailbox::Filed(m) | HeldMailbox::Review { inbox: m } => self.inner.watch(m),
        }
    }

    async fn trashed(&self) -> Result<Trashed<Self::Mailbox, Self::Key>> {
        Ok(shown(self.inner.trashed().await?))
    }

    async fn held(&self) -> Result<Trashed<Self::Mailbox, Self::Key>> {
        Ok(shown(self.inner.held().await?))
    }

    async fn archived(&self) -> Result<Trashed<Self::Mailbox, Self::Key>> {
        Ok(shown(self.inner.archived().await?))
    }

    async fn all_mail(&self) -> Result<Trashed<Self::Mailbox, Self::Key>> {
        Ok(shown(self.inner.all_mail().await?))
    }

    async fn set_archived(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        archived: bool,
    ) -> Result<bool> {
        let (msg, filed) = unwrap(message);
        match Self::filed_in(mailbox, &filed) {
            Some(m) => self.inner.set_archived(m, &msg, archived).await,
            None => Err(Error::Store("not a message shown here".into())),
        }
    }

    async fn set_trashed(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        trashed: bool,
    ) -> Result<bool> {
        let (msg, filed) = unwrap(message);
        match Self::filed_in(mailbox, &filed) {
            Some(m) => self.inner.set_trashed(m, &msg, trashed).await,
            None => Err(Error::Store("not a message shown here".into())),
        }
    }

    async fn labels(&self) -> Result<Vec<LabelFolder<Self::Mailbox, Self::Key>>> {
        Ok(self
            .inner
            .labels()
            .await?
            .into_iter()
            .map(|l| LabelFolder {
                id: l.id,
                name: l.name,
                messages: l
                    .messages
                    .into_iter()
                    .map(|t| ShownMessage {
                        mailbox: HeldMailbox::Filed(t.mailbox),
                        uid: t.uid,
                        message: wrap(t.message),
                    })
                    .collect(),
            })
            .collect())
    }

    async fn set_label(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        id: &str,
        on: bool,
    ) -> Result<bool> {
        let (msg, filed) = unwrap(message);
        match Self::filed_in(mailbox, &filed) {
            Some(m) => self.inner.set_label(m, &msg, id, on).await,
            None => Err(Error::Store("not a message shown here".into())),
        }
    }

    async fn sieve_script(&self) -> Result<Option<String>> {
        self.inner.sieve_script().await
    }

    async fn put_sieve_script(&self, body: &str) -> Result<()> {
        self.inner.put_sieve_script(body).await
    }

    async fn delete_sieve_script(&self) -> Result<()> {
        self.inner.delete_sieve_script().await
    }
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
