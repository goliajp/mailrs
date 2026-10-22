//! A signed-in fastcore user: a real account, or a mock namespace that
//! can only be read.

use std::path::PathBuf;

use mailrs_mail_protocols::backend::{
    Account, Error, Expunged, Flag, Message, Quota, Result, Selected,
};

use super::mock::MockAccount;
use super::real::RealAccount;
use super::watch::FeedWatch;
use crate::imap::backend::{ImapMessage, MailboxInfo};

pub enum FastcoreAccount {
    Real(RealAccount),
    Mock(MockAccount),
}

pub(crate) type FcMailbox = MailboxInfo;

pub(crate) const MOCK_READ_ONLY: &str = "[READ-ONLY] mock mailboxes are read-only";

fn refused<T>() -> Result<T> {
    Err(Error::Refused(MOCK_READ_ONLY.into()))
}

pub(crate) fn to_message(m: ImapMessage) -> Message<PathBuf> {
    Message {
        uid: m.uid,
        seqno: m.seqno,
        flags: m.flags,
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
        key: m.path,
    }
}

pub(crate) fn to_imap(m: &Message<PathBuf>) -> ImapMessage {
    ImapMessage {
        uid: m.uid,
        seqno: m.seqno,
        path: m.key.clone(),
        flags: m.flags.clone(),
        internal_date: m.internal_date,
        size: m.size,
        modseq: m.modseq,
    }
}

/// The message file; `None` when it is gone.
fn read_file(m: &Message<PathBuf>) -> Result<Option<Vec<u8>>> {
    match std::fs::read(&m.key) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

impl Account for FastcoreAccount {
    type Mailbox = FcMailbox;
    type Key = PathBuf;
    type Watch = FeedWatch;

    fn user(&self) -> &str {
        match self {
            Self::Real(a) => &a.user,
            Self::Mock(a) => &a.user,
        }
    }

    fn read_only(&self) -> Option<&str> {
        match self {
            Self::Real(_) => None,
            Self::Mock(_) => Some(MOCK_READ_ONLY),
        }
    }

    async fn mailbox_names(&self) -> Result<Vec<String>> {
        match self {
            Self::Real(a) => Ok(a.mailbox_names()),
            Self::Mock(_) => Ok(vec!["INBOX".into()]),
        }
    }

    async fn mailbox(&self, name: &str) -> Result<Option<FcMailbox>> {
        match self {
            Self::Real(a) => Ok(a.mailbox(name)),
            Self::Mock(a) => a.mailbox(name).map(Some),
        }
    }

    fn mailbox_name<'a>(&self, mailbox: &'a FcMailbox) -> &'a str {
        &mailbox.name
    }

    async fn select(&self, mailbox: &FcMailbox) -> Result<Selected<PathBuf>> {
        match self {
            Self::Real(a) => Ok(a.select(mailbox)),
            Self::Mock(a) => a.select().await,
        }
    }

    async fn messages(&self, mailbox: &FcMailbox) -> Result<Vec<Message<PathBuf>>> {
        match self {
            Self::Real(a) => Ok(a.messages(mailbox)),
            Self::Mock(a) => Ok(a.select().await?.messages),
        }
    }

    async fn read(&self, message: &Message<PathBuf>) -> Result<Option<Vec<u8>>> {
        read_file(message)
    }

    async fn store_flags(&self, _: &FcMailbox, m: &Message<PathBuf>, flags: &[Flag]) -> Result<()> {
        match self {
            Self::Real(a) => a.store_flags(m, flags),
            Self::Mock(_) => refused(),
        }
    }

    async fn copy(&self, _: &FcMailbox, m: &Message<PathBuf>, to: &FcMailbox) -> Result<()> {
        match self {
            Self::Real(a) => a.copy(m, to),
            Self::Mock(_) => refused(),
        }
    }

    async fn move_to(&self, from: &FcMailbox, m: &Message<PathBuf>, to: &FcMailbox) -> Result<()> {
        match self {
            Self::Real(a) => a.move_to(from, m, to),
            Self::Mock(_) => refused(),
        }
    }

    async fn expunge(&self, mailbox: &FcMailbox, doomed: &[Message<PathBuf>]) -> Expunged {
        match self {
            Self::Real(a) => a.expunge(mailbox, doomed),
            Self::Mock(_) => Expunged {
                removed: Vec::new(),
                failed: refused::<()>().err(),
            },
        }
    }

    fn unique_id(&self, m: &Message<PathBuf>) -> String {
        m.key
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| m.uid.to_string())
    }

    async fn remove(&self, _: &FcMailbox, m: &Message<PathBuf>) -> Result<()> {
        match self {
            Self::Real(_) => Ok(std::fs::remove_file(&m.key)?),
            Self::Mock(_) => refused(),
        }
    }

    async fn append(&self, mailbox: &FcMailbox, bytes: Vec<u8>, _flags: Vec<Flag>) -> Result<()> {
        match self {
            Self::Real(a) => a.append(mailbox, &bytes),
            Self::Mock(_) => refused(),
        }
    }

    async fn vanished_since(&self, mailbox: &FcMailbox, modseq: u64) -> Result<Vec<u32>> {
        match self {
            Self::Real(a) => Ok(a.vanished_since(mailbox, modseq)),
            Self::Mock(_) => Ok(Vec::new()),
        }
    }

    async fn quota(&self) -> Result<Quota> {
        let (limit, used) = crate::live_sync::quota_read(self.user());
        Ok(Quota {
            used: used.max(0) as u64,
            limit: limit.max(0) as u64,
        })
    }

    fn watch(&self, _: &FcMailbox) -> FeedWatch {
        match self {
            Self::Real(a) => FeedWatch::new(a.state.clone(), &a.user),
            Self::Mock(_) => FeedWatch::never(),
        }
    }

    async fn sieve_script(&self) -> Result<Option<String>> {
        match self {
            Self::Real(a) => Ok(super::sieve::load_script(&a.state, &a.user)?),
            Self::Mock(_) => refused(),
        }
    }

    async fn put_sieve_script(&self, body: &str) -> Result<()> {
        match self {
            Self::Real(a) => Ok(super::sieve::store_script(&a.state, &a.user, body)?),
            Self::Mock(_) => refused(),
        }
    }

    async fn delete_sieve_script(&self) -> Result<()> {
        match self {
            Self::Real(a) => Ok(super::sieve::delete_script(&a.state, &a.user)?),
            Self::Mock(_) => refused(),
        }
    }
}
