//! What a session needs from a core's storage.
//!
//! A core implements [`Directory`] (who may sign in) and [`Account`] (one
//! signed-in user's mailboxes and Sieve script). Every protocol answer is
//! built by the sessions in this crate; an implementation only reads and
//! writes its store, and reports what it could not do as an [`Error`].

use std::future::Future;

pub use mailrs_maildir::Flag;

/// A store failure, or a refusal the client is to be shown verbatim.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Sent after `NO` as is, response code included
    /// (`[READ-ONLY] …`, `[UNAVAILABLE] …`).
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A store's own error, as text.
    #[error("{0}")]
    Store(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Which listener a sign-in arrived on; a directory may admit different
/// people to each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Imap,
    Pop3,
    ManageSieve,
}

/// The accounts a core can sign in.
pub trait Directory: Send + Sync + 'static {
    type Account: Account;

    /// The name the IMAP greeting gives.
    fn hostname(&self) -> String;

    /// `Ok(None)` for wrong credentials.
    fn login(
        &self,
        user: &str,
        password: &str,
        protocol: Protocol,
    ) -> impl Future<Output = Result<Option<Self::Account>>> + Send;
}

/// One message of a mailbox, as a session sees it.
#[derive(Debug, Clone)]
pub struct Message<K> {
    /// Ascending with `seqno` (RFC 3501 §2.3.1.1), never reused.
    pub uid: u32,
    /// 1-based position in the mailbox, assigned by the store's listing.
    pub seqno: u32,
    pub flags: Vec<Flag>,
    /// Unix seconds.
    pub internal_date: i64,
    pub size: u64,
    /// CONDSTORE modification sequence (RFC 7162).
    pub modseq: u64,
    /// Where the store keeps it.
    pub key: K,
}

/// What SELECT and EXAMINE report.
#[derive(Debug, Clone)]
pub struct Selected<K> {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highest_modseq: u64,
    pub recent: u32,
    pub messages: Vec<Message<K>>,
}

/// Bytes, for GETQUOTA. A `limit` of 0 means there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub used: u64,
    pub limit: u64,
}

/// The outcome of an EXPUNGE: the uids that are gone, in the order they
/// went, and the failure that stopped the rest.
#[derive(Debug, Default)]
pub struct Expunged {
    pub removed: Vec<u32>,
    pub failed: Option<Error>,
}

/// Where a message was filed by APPEND, COPY or MOVE: the destination's
/// UIDVALIDITY and the uid the message has there (RFC 4315).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filed {
    pub uidvalidity: u32,
    pub uid: u32,
}

/// Resolves when the watched mailbox may have changed (IMAP IDLE).
pub trait Watch: Send {
    fn changed(&mut self) -> impl Future<Output = ()> + Send;
}

/// One signed-in user's mail.
pub trait Account: Send + Sync + 'static {
    /// A mailbox the store found by name.
    type Mailbox: Clone + Send + Sync + 'static;
    /// What [`Message::key`] holds.
    type Key: Clone + Send + Sync + 'static;
    type Watch: Watch;

    /// The address the session acts for.
    fn user(&self) -> &str;

    /// `Some(reason)` when this account can change nothing; every
    /// changing command is then answered `NO reason`.
    fn read_only(&self) -> Option<&str> {
        None
    }

    /// Every mailbox name, in the order LIST gives them.
    fn mailbox_names(&self) -> impl Future<Output = Result<Vec<String>>> + Send;

    /// `name` as a client wrote it; INBOX in any case.
    fn mailbox(&self, name: &str) -> impl Future<Output = Result<Option<Self::Mailbox>>> + Send;

    /// The mailbox's own name, which is what UIDVALIDITY and the
    /// vanished log are kept under.
    fn mailbox_name<'a>(&self, mailbox: &'a Self::Mailbox) -> &'a str;

    fn select(
        &self,
        mailbox: &Self::Mailbox,
    ) -> impl Future<Output = Result<Selected<Self::Key>>> + Send;

    /// The mailbox's messages now, ascending by uid.
    fn messages(
        &self,
        mailbox: &Self::Mailbox,
    ) -> impl Future<Output = Result<Vec<Message<Self::Key>>>> + Send;

    /// `Ok(None)` when the message is gone.
    fn read(
        &self,
        message: &Message<Self::Key>,
    ) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send;

    /// Replace the message's flags with `flags`.
    fn store_flags(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
        flags: &[Flag],
    ) -> impl Future<Output = Result<()>> + Send;

    fn copy(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> impl Future<Output = Result<Filed>> + Send;

    fn move_to(
        &self,
        from: &Self::Mailbox,
        message: &Message<Self::Key>,
        to: &Self::Mailbox,
    ) -> impl Future<Output = Result<Filed>> + Send;

    /// Remove `doomed`, highest seqno first.
    fn expunge(
        &self,
        mailbox: &Self::Mailbox,
        doomed: &[Message<Self::Key>],
    ) -> impl Future<Output = Expunged> + Send;

    /// The POP3 UIDL unique-id (RFC 1939 §7).
    fn unique_id(&self, message: &Message<Self::Key>) -> String;

    /// A POP3 deletion: the message leaves the mailbox.
    fn remove(
        &self,
        mailbox: &Self::Mailbox,
        message: &Message<Self::Key>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// A new message carrying `flags`.
    fn append(
        &self,
        mailbox: &Self::Mailbox,
        bytes: Vec<u8>,
        flags: Vec<Flag>,
    ) -> impl Future<Output = Result<Filed>> + Send;

    /// Uids expunged after `modseq`, ascending (QRESYNC).
    fn vanished_since(
        &self,
        mailbox: &Self::Mailbox,
        modseq: u64,
    ) -> impl Future<Output = Result<Vec<u32>>> + Send;

    fn quota(&self) -> impl Future<Output = Result<Quota>> + Send;

    fn watch(&self, mailbox: &Self::Mailbox) -> Self::Watch;

    /// The user's Sieve script; `Ok(None)` when there is none.
    fn sieve_script(&self) -> impl Future<Output = Result<Option<String>>> + Send;

    fn put_sieve_script(&self, body: &str) -> impl Future<Output = Result<()>> + Send;

    fn delete_sieve_script(&self) -> impl Future<Output = Result<()>> + Send;
}
