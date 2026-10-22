//! One user's mail in the SQL store: mailbox and message rows in
//! [`PgMailboxStore`], bodies in the user's Maildir, read and written
//! through the [`MessageStore`] seam.

use std::sync::Arc;

use mailrs_mail_protocols::backend::{
    Account, Error, Expunged, Filed, Flag, Message, Quota, Result, Selected,
};
use mailrs_mailbox::{
    FLAG_DELETED, Mailbox, MessageMeta, PgMailboxStore, bitmask_to_maildir_flags,
    maildir_flags_to_bitmask,
};

use super::PgDirectory;
use super::watch::BusWatch;
use crate::domain_store::DomainStore;
use crate::event_bus::EventBus;
use crate::message_store::MessageStore;
use crate::sieve_store;

mod filing;

/// What a user may store when the account row names no quota.
const DEFAULT_QUOTA: u64 = 1 << 30;

pub(crate) struct PgAccount {
    user: String,
    mailbox: Arc<PgMailboxStore>,
    domain: Option<Arc<DomainStore>>,
    message_store: Arc<dyn MessageStore>,
    events: EventBus,
    maildir_root: String,
    net_url: Option<String>,
}

fn store<E: std::fmt::Display>(e: E) -> Error {
    Error::Store(e.to_string())
}

/// The copy of `m` in `to`; `None` when `m` was gone.
pub(super) fn filed(to: &Mailbox, uid: Option<u32>, m: &Message<String>) -> Result<Filed> {
    let uid = uid.ok_or_else(|| Error::Store(format!("message {} is gone", m.uid)))?;
    Ok(Filed {
        uidvalidity: to.uidvalidity,
        uid,
    })
}

fn to_message(seqno: u32, m: MessageMeta) -> Message<String> {
    Message {
        uid: m.uid,
        seqno,
        flags: bitmask_to_maildir_flags(m.flags),
        internal_date: m.internal_date,
        size: u64::from(m.size),
        modseq: m.modseq,
        key: m.maildir_id,
    }
}

impl PgAccount {
    pub(super) fn new(dir: &PgDirectory, user: &str) -> Self {
        Self {
            user: user.to_string(),
            mailbox: dir.mailbox.clone(),
            domain: dir.domain.clone(),
            message_store: dir.message_store.clone(),
            events: dir.events.clone(),
            maildir_root: dir.maildir_root.clone(),
            net_url: dir.net_url.clone(),
        }
    }

    /// INBOX first, then by name, as fastcore lists them.
    async fn mailboxes(&self) -> Result<Vec<Mailbox>> {
        let mut all = self
            .mailbox
            .list_mailboxes(&self.user)
            .await
            .map_err(store)?;
        all.sort_by(|a, b| (a.name != "INBOX", &a.name).cmp(&(b.name != "INBOX", &b.name)));
        Ok(all)
    }
}

impl Account for PgAccount {
    type Mailbox = Mailbox;
    type Key = String;
    type Watch = BusWatch;

    fn user(&self) -> &str {
        &self.user
    }

    async fn mailbox_names(&self) -> Result<Vec<String>> {
        Ok(self
            .mailboxes()
            .await?
            .into_iter()
            .map(|m| m.name)
            .collect())
    }

    /// INBOX in any case; the others as named, ignoring case.
    async fn mailbox(&self, name: &str) -> Result<Option<Mailbox>> {
        let all = self.mailboxes().await?;
        Ok(all.into_iter().find(|m| m.name.eq_ignore_ascii_case(name)))
    }

    fn mailbox_name<'a>(&self, mailbox: &'a Mailbox) -> &'a str {
        &mailbox.name
    }

    async fn select(&self, mailbox: &Mailbox) -> Result<Selected<String>> {
        // the row as it is now: uidnext and modseq move with every arrival
        let current = self
            .mailbox
            .get_mailbox_by_id(mailbox.id)
            .await
            .map_err(store)?
            .ok_or_else(|| Error::Store(format!("mailbox {} is gone", mailbox.name)))?;
        let messages = self.messages(&current).await?;
        Ok(Selected {
            uidvalidity: current.uidvalidity,
            uidnext: current.uidnext,
            highest_modseq: current.highest_modseq,
            // as fastcore: no message has been seen by a session yet
            recent: messages.len() as u32,
            messages,
        })
    }

    async fn messages(&self, mailbox: &Mailbox) -> Result<Vec<Message<String>>> {
        let rows = self
            .mailbox
            .list_messages(mailbox.id, 0, u32::MAX)
            .await
            .map_err(store)?;
        Ok((1u32..).zip(rows).map(|(n, m)| to_message(n, m)).collect())
    }

    async fn read(&self, message: &Message<String>) -> Result<Option<Vec<u8>>> {
        let store = self.message_store.as_ref();
        Ok(crate::body::read(store, &self.maildir_root, &self.user, &message.key).await?)
    }

    async fn store_flags(
        &self,
        mailbox: &Mailbox,
        m: &Message<String>,
        flags: &[Flag],
    ) -> Result<()> {
        let bits = maildir_flags_to_bitmask(flags);
        self.mailbox
            .update_flags(mailbox.id, m.uid, bits)
            .await
            .map_err(store)?;
        Ok(())
    }

    async fn copy(&self, from: &Mailbox, m: &Message<String>, to: &Mailbox) -> Result<Filed> {
        self.file(from, m, to, false).await
    }

    async fn move_to(&self, from: &Mailbox, m: &Message<String>, to: &Mailbox) -> Result<Filed> {
        self.file(from, m, to, true).await
    }

    /// The rows of `doomed` go, and no others, and each takes its body with
    /// it: every row has a body of its own.
    async fn expunge(&self, mailbox: &Mailbox, doomed: &[Message<String>]) -> Expunged {
        let uids: Vec<u32> = doomed.iter().map(|m| m.uid).collect();
        let removed = match self.mailbox.expunge_uids(mailbox.id, &uids).await {
            Ok(removed) => removed,
            Err(e) => {
                return Expunged {
                    removed: Vec::new(),
                    failed: Some(store(e)),
                };
            }
        };
        let failed = self.remove_bodies(doomed, &removed).await.err();
        Expunged { removed, failed }
    }

    /// The uid, which a message keeps for as long as it is in INBOX.
    fn unique_id(&self, m: &Message<String>) -> String {
        m.uid.to_string()
    }

    async fn remove(&self, mailbox: &Mailbox, m: &Message<String>) -> Result<()> {
        self.mailbox
            .add_flags(mailbox.id, m.uid, FLAG_DELETED)
            .await
            .map_err(store)?;
        let removed = self
            .mailbox
            .expunge_uids(mailbox.id, &[m.uid])
            .await
            .map_err(store)?;
        self.remove_bodies(std::slice::from_ref(m), &removed).await
    }

    async fn append(&self, mailbox: &Mailbox, bytes: Vec<u8>, flags: Vec<Flag>) -> Result<Filed> {
        let uid = self
            .deliver(mailbox, &bytes, maildir_flags_to_bitmask(&flags))
            .await?;
        Ok(Filed {
            uidvalidity: mailbox.uidvalidity,
            uid,
        })
    }

    /// No log of expunged uids is kept, so QRESYNC never reports any.
    async fn vanished_since(&self, _: &Mailbox, _: u64) -> Result<Vec<u32>> {
        Ok(Vec::new())
    }

    async fn quota(&self) -> Result<Quota> {
        let used = self.mailbox.user_storage_usage(&self.user).await;
        let limit = match &self.domain {
            Some(ds) => ds.get_quota(&self.user).await.map_err(store)?,
            None => None,
        };
        Ok(Quota {
            used,
            limit: limit.map_or(DEFAULT_QUOTA, |q| q.max(0) as u64),
        })
    }

    fn watch(&self, _: &Mailbox) -> BusWatch {
        BusWatch::new(&self.events, &self.user)
    }

    async fn sieve_script(&self) -> Result<Option<String>> {
        Ok(sieve_store::get_at(self.net_url()?, &self.user)?)
    }

    async fn put_sieve_script(&self, body: &str) -> Result<()> {
        Ok(sieve_store::set_at(self.net_url()?, &self.user, body)?)
    }

    async fn delete_sieve_script(&self) -> Result<()> {
        sieve_store::delete_at(self.net_url()?, &self.user)?;
        Ok(())
    }
}

impl PgAccount {
    /// The network kevy holding the script, which every reader of it shares.
    fn net_url(&self) -> Result<&str> {
        Ok(self
            .net_url
            .as_deref()
            .ok_or_else(sieve_store::unreachable)?)
    }
}
