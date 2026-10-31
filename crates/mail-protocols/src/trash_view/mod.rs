//! The Trash as IMAP and POP3 see it.
//!
//! The web, the apps and MCP move whole conversations to the Trash without
//! moving a file. This view shows that to mail clients the way they expect a
//! Trash: a message of a conversation in the Trash leaves every other
//! mailbox and appears in `Trash`; moving a message into `Trash` moves its
//! conversation there; moving it out brings the conversation back; and
//! expunging it in `Trash` deletes it for good. A core only says which
//! messages are in the Trash and moves conversations in and out
//! ([`Account::trashed`], [`Account::set_trashed`]).

use std::collections::HashSet;

mod filing;
mod labels;

use crate::backend::{
    Account, Error, Expunged, Filed, Flag, LabelFolder, Message, Quota, Result, Selected,
    ShownMessage, Trashed,
};

/// The name `Trash` goes by, in every case a client may write it.
const TRASH: &str = "Trash";

/// A signed-in account, with `Trash` made of its conversations in the Trash.
pub struct TrashView<A> {
    inner: A,
}

impl<A> TrashView<A> {
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

/// A mailbox of the view.
#[derive(Clone)]
pub enum ViewMailbox<M> {
    Filed(M),
    /// `inbox` is what IDLE in `Trash` watches: the stores signal a change
    /// per account, not per mailbox.
    Trash {
        inbox: M,
    },
    /// A label, with the same `inbox` for IDLE.
    Label {
        id: String,
        name: String,
        inbox: M,
    },
}

/// A message of the view: the store's key, and in `Trash` where the message
/// is filed and the uid it has there.
#[derive(Clone)]
pub struct ViewKey<M, K> {
    key: K,
    filed: Option<(M, u32)>,
}

type Inner<A> = Message<<A as Account>::Key>;

fn wrap<M, K>(m: Message<K>) -> Message<ViewKey<M, K>> {
    Message {
        uid: m.uid,
        seqno: m.seqno,
        flags: m.flags,
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: ViewKey {
            key: m.key,
            filed: None,
        },
    }
}

/// The message as its store knows it, and the mailbox it is filed in when
/// that is not `mailbox`.
pub(super) fn unwrap<M: Clone, K: Clone>(m: &Message<ViewKey<M, K>>) -> (Message<K>, Option<M>) {
    let (uid, filed) = match &m.key.filed {
        Some((mb, uid)) => (*uid, Some(mb.clone())),
        None => (m.uid, None),
    };
    let msg = Message {
        uid,
        seqno: m.seqno,
        flags: m.flags.clone(),
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: m.key.key.clone(),
    };
    (msg, filed)
}

pub(super) fn renumber<K>(messages: &mut [Message<K>]) {
    for (i, m) in messages.iter_mut().enumerate() {
        m.seqno = i as u32 + 1;
    }
}

fn is_trash(name: &str) -> bool {
    name.eq_ignore_ascii_case(TRASH)
}

/// `Trash`'s UIDVALIDITY, from when the latest conversation went there.
/// Never 0, which RFC 3501 does not allow, and different for an empty
/// Trash than for any moment something went there.
fn trash_uidvalidity(latest: i64) -> u32 {
    u32::try_from(latest)
        .unwrap_or(u32::MAX - 1)
        .min(u32::MAX - 1)
        + 1
}

impl<A: Account> TrashView<A> {
    /// Which messages of `mailbox` are in the Trash, by uid.
    async fn hidden(&self, mailbox: &A::Mailbox) -> Result<HashSet<u32>> {
        let name = self.inner.mailbox_name(mailbox).to_string();
        let trashed = self.inner.trashed().await?;
        Ok(trashed
            .messages
            .iter()
            .filter(|t| self.inner.mailbox_name(&t.mailbox) == name)
            .map(|t| t.message.uid)
            .collect())
    }

    async fn filed_messages(&self, mailbox: &A::Mailbox) -> Result<Vec<Inner<A>>> {
        let hidden = self.hidden(mailbox).await?;
        let mut kept: Vec<_> = self
            .inner
            .messages(mailbox)
            .await?
            .into_iter()
            .filter(|m| !hidden.contains(&m.uid))
            .collect();
        renumber(&mut kept);
        Ok(kept)
    }

    async fn trash_messages(
        &self,
    ) -> Result<(
        Trashed<A::Mailbox, A::Key>,
        Vec<Message<ViewKey<A::Mailbox, A::Key>>>,
    )> {
        let trashed = self.inner.trashed().await?;
        let mut out: Vec<_> = trashed.messages.iter().map(trash_entry).collect();
        out.sort_by_key(|m| m.uid);
        renumber(&mut out);
        Ok((trashed, out))
    }

    async fn inbox(&self) -> Result<A::Mailbox> {
        self.inner
            .mailbox("INBOX")
            .await?
            .ok_or_else(|| Error::Store("no INBOX".into()))
    }
}

pub(super) fn trash_entry<M: Clone, K: Clone>(t: &ShownMessage<M, K>) -> Message<ViewKey<M, K>> {
    let m = &t.message;
    Message {
        uid: t.uid,
        seqno: 0,
        flags: m.flags.clone(),
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: ViewKey {
            key: m.key.clone(),
            filed: Some((t.mailbox.clone(), m.uid)),
        },
    }
}

impl<A: Account> Account for TrashView<A> {
    type Mailbox = ViewMailbox<A::Mailbox>;
    type Key = ViewKey<A::Mailbox, A::Key>;
    type Watch = A::Watch;

    fn user(&self) -> &str {
        self.inner.user()
    }

    fn read_only(&self) -> Option<&str> {
        self.inner.read_only()
    }

    async fn mailbox_names(&self) -> Result<Vec<String>> {
        let mut names = self.inner.mailbox_names().await?;
        // an account that can change nothing has no Trash or labels
        if self.inner.read_only().is_some() {
            return Ok(names);
        }
        if !names.iter().any(|n| is_trash(n)) {
            names.push(TRASH.into());
        }
        // a label takes over a mailbox of its name
        for label in self.inner.labels().await? {
            names.retain(|n| !n.eq_ignore_ascii_case(&label.name));
            names.push(label.name);
        }
        Ok(names)
    }

    async fn mailbox(&self, name: &str) -> Result<Option<Self::Mailbox>> {
        if is_trash(name) && self.inner.read_only().is_none() {
            return Ok(Some(ViewMailbox::Trash {
                inbox: self.inbox().await?,
            }));
        }
        if let Some(label) = self.label_mailbox(name).await? {
            return Ok(Some(label));
        }
        Ok(self.inner.mailbox(name).await?.map(ViewMailbox::Filed))
    }

    fn mailbox_name<'a>(&self, mailbox: &'a Self::Mailbox) -> &'a str {
        match mailbox {
            ViewMailbox::Filed(m) => self.inner.mailbox_name(m),
            ViewMailbox::Trash { .. } => TRASH,
            ViewMailbox::Label { name, .. } => name,
        }
    }

    async fn select(&self, mailbox: &Self::Mailbox) -> Result<Selected<Self::Key>> {
        match mailbox {
            ViewMailbox::Filed(m) => {
                let selected = self.inner.select(m).await?;
                let messages: Vec<_> = self
                    .filed_messages(m)
                    .await?
                    .into_iter()
                    .map(wrap)
                    .collect();
                Ok(Selected {
                    uidvalidity: selected.uidvalidity,
                    uidnext: selected.uidnext,
                    highest_modseq: selected.highest_modseq,
                    recent: messages.len() as u32,
                    messages,
                })
            }
            ViewMailbox::Trash { .. } => {
                let (trashed, messages) = self.trash_messages().await?;
                Ok(Selected {
                    uidvalidity: trash_uidvalidity(trashed.latest),
                    uidnext: messages.iter().map(|m| m.uid).max().unwrap_or(0) + 1,
                    highest_modseq: messages.iter().map(|m| m.modseq).max().unwrap_or(0),
                    recent: messages.len() as u32,
                    messages,
                })
            }
            ViewMailbox::Label { id, .. } => self.label_selected(id).await,
        }
    }

    async fn messages(&self, mailbox: &Self::Mailbox) -> Result<Vec<Message<Self::Key>>> {
        match mailbox {
            ViewMailbox::Filed(m) => Ok(self
                .filed_messages(m)
                .await?
                .into_iter()
                .map(wrap)
                .collect()),
            ViewMailbox::Trash { .. } => Ok(self.trash_messages().await?.1),
            ViewMailbox::Label { id, .. } => self.label_messages(id).await,
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
        match (mailbox, filed) {
            (_, Some(from)) => self.inner.store_flags(&from, &msg, flags).await,
            (ViewMailbox::Filed(m), None) => self.inner.store_flags(m, &msg, flags).await,
            (ViewMailbox::Trash { .. } | ViewMailbox::Label { .. }, None) => {
                Err(Error::Store("not a message shown here".into()))
            }
        }
    }

    async fn copy(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> Result<Filed> {
        self.file(from, message, to, true).await
    }

    async fn move_to(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> Result<Filed> {
        self.file(from, message, to, false).await
    }

    async fn expunge(&self, mailbox: &Self::Mailbox, doomed: &[Message<Self::Key>]) -> Expunged {
        match mailbox {
            ViewMailbox::Filed(m) => self.expunge_filed(m, doomed).await,
            ViewMailbox::Trash { .. } => self.expunge_trash(doomed).await,
            ViewMailbox::Label { id, .. } => self.expunge_label(id, doomed).await,
        }
    }

    fn unique_id(&self, message: &Message<Self::Key>) -> String {
        self.inner.unique_id(&unwrap(message).0)
    }

    async fn remove(&self, mailbox: &Self::Mailbox, message: &Message<Self::Key>) -> Result<()> {
        let (msg, filed) = unwrap(message);
        match (mailbox, filed) {
            (_, Some(from)) => self.inner.remove(&from, &msg).await,
            (ViewMailbox::Filed(m), None) => self.inner.remove(m, &msg).await,
            (ViewMailbox::Trash { .. } | ViewMailbox::Label { .. }, None) => {
                Err(Error::Store("not a message shown here".into()))
            }
        }
    }

    async fn append(
        &self,
        mailbox: &Self::Mailbox,
        bytes: Vec<u8>,
        flags: Vec<Flag>,
    ) -> Result<Filed> {
        match mailbox {
            ViewMailbox::Filed(m) => self.inner.append(m, bytes, flags).await,
            ViewMailbox::Trash { .. } | ViewMailbox::Label { .. } => Err(Error::Refused(
                "[CANNOT] messages reach the Trash or a label by being copied there".into(),
            )),
        }
    }

    async fn vanished_since(&self, mailbox: &Self::Mailbox, modseq: u64) -> Result<Vec<u32>> {
        match mailbox {
            // what went to the Trash left this mailbox too; reporting it
            // whenever asked is harmless, a client drops uids it never had
            ViewMailbox::Filed(m) => {
                let mut gone = self.inner.vanished_since(m, modseq).await?;
                gone.extend(self.hidden(m).await?);
                gone.sort_unstable();
                gone.dedup();
                Ok(gone)
            }
            ViewMailbox::Trash { .. } | ViewMailbox::Label { .. } => Ok(Vec::new()),
        }
    }

    async fn quota(&self) -> Result<Quota> {
        self.inner.quota().await
    }

    fn watch(&self, mailbox: &Self::Mailbox) -> Self::Watch {
        match mailbox {
            ViewMailbox::Filed(m)
            | ViewMailbox::Trash { inbox: m }
            | ViewMailbox::Label { inbox: m, .. } => self.inner.watch(m),
        }
    }

    async fn trashed(&self) -> Result<Trashed<Self::Mailbox, Self::Key>> {
        let trashed = self.inner.trashed().await?;
        Ok(Trashed {
            messages: trashed
                .messages
                .into_iter()
                .map(|t| ShownMessage {
                    mailbox: ViewMailbox::Filed(t.mailbox),
                    uid: t.uid,
                    message: wrap(t.message),
                })
                .collect(),
            latest: trashed.latest,
        })
    }

    async fn set_trashed(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        trashed: bool,
    ) -> Result<bool> {
        let (msg, filed) = unwrap(message);
        match (mailbox, filed) {
            (_, Some(from)) => self.inner.set_trashed(&from, &msg, trashed).await,
            (ViewMailbox::Filed(m), None) => self.inner.set_trashed(m, &msg, trashed).await,
            (ViewMailbox::Trash { .. } | ViewMailbox::Label { .. }, None) => {
                Err(Error::Store("not a message shown here".into()))
            }
        }
    }

    async fn labels(&self) -> Result<Vec<LabelFolder<Self::Mailbox, Self::Key>>> {
        let labels = self.inner.labels().await?;
        Ok(labels
            .into_iter()
            .map(|l| LabelFolder {
                id: l.id,
                name: l.name,
                messages: l
                    .messages
                    .into_iter()
                    .map(|t| ShownMessage {
                        mailbox: ViewMailbox::Filed(t.mailbox),
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
        match (mailbox, filed) {
            (_, Some(from)) => self.inner.set_label(&from, &msg, id, on).await,
            (ViewMailbox::Filed(m), None) => self.inner.set_label(m, &msg, id, on).await,
            (ViewMailbox::Trash { .. } | ViewMailbox::Label { .. }, None) => {
                Err(Error::Store("not a message shown here".into()))
            }
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
mod tests;
