//! What re-judging needs of a store. Synchronous, as the re-judge runs on a
//! thread of its own; a store that awaits its database blocks on it here.
//!
//! The re-judge's own state (where it stands, what each round gathered,
//! the batches) is a handful of small records named by key: strings, hash
//! fields and sets, under the key names the kevy store has always used,
//! so state already written stays readable.

use std::io;

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_sidestate::NetKevy;
use mailrs_mailbox::ledger::LedgerVerdict;

use crate::fraud_judge::{Extracted, FraudJudge};

/// A reader's conversation, as re-judging weighs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conversation {
    /// Held for review.
    pub held: bool,
    /// Filed in Junk.
    pub in_junk: bool,
}

/// Reads stored messages, one page of a round at a time: a store may keep
/// what it learned about a reader's files between messages.
pub trait RawReader {
    /// `user`'s copy of message `mid`, as stored; `None` when its file is
    /// gone.
    fn raw(&mut self, user: &str, mid: &str) -> Option<Vec<u8>>;
}

pub trait RejudgeStore: NetKevy {
    type Reader<'a>: RawReader
    where
        Self: 'a;

    fn raw_reader(&self) -> Self::Reader<'_>;

    /// The domains this server receives mail for, lowercased.
    fn local_domains(&self) -> Vec<String>;
    /// This deployment's `authserv-id`.
    fn authserv_id(&self) -> Option<String>;
    /// The fraud rules as this process is configured.
    fn fraud_judge(&self) -> FraudJudge;
    /// What message `mid`'s attachments were read to say.
    fn extracted(&self, mid: &str) -> Extracted;

    /// The ledger's `(message id, user)` of mail from `domain` or below it.
    fn ledger_by_domain_tree(&self, domain: &str, reg: &str) -> io::Result<Vec<(String, String)>>;
    /// The ledger's `(message id, user)` of mail linking to `host` or below.
    fn ledger_by_link_tree(&self, host: &str, reg: &str) -> io::Result<Vec<(String, String)>>;
    fn ledger_verdict(&self, user: &str, mid: &str) -> io::Result<Option<LedgerVerdict>>;
    fn record_verdict(&self, user: &str, mid: &str, v: &LedgerVerdict) -> io::Result<()>;

    /// The conversation `mid` is in for `user`, and its state.
    fn thread_of(&self, user: &str, mid: &str) -> Option<(String, Conversation)>;
    /// `user`'s conversation `tid`; `None` when they have none.
    fn conversation(&self, user: &str, tid: &str) -> io::Result<Option<Conversation>>;
    /// The messages of `user`'s conversation `tid`.
    fn thread_message_ids(&self, user: &str, tid: &str) -> io::Result<Vec<String>>;
    /// The fraud verdict stored beside message `mid`, as JSON.
    fn fraud_verdict(&self, mid: &str) -> io::Result<Option<String>>;
    fn set_fraud_verdict(&self, mid: &str, json: &str) -> io::Result<()>;

    /// Hold `tid` for `user`, appending its hold fact (`source`, the rule
    /// pack, what was unread). False when the conversation is gone.
    fn hold(&self, user: &str, tid: &str, source: &str, rules_version: &str) -> io::Result<bool>;
    /// Let `tid` go for `user`. False when the conversation is gone.
    fn release(&self, user: &str, tid: &str) -> io::Result<bool>;
    /// The source of the newest hold of `tid` for `user`.
    fn last_hold_source(&self, user: &str, tid: &str) -> io::Result<Option<String>>;
    /// Every release of `tid` for `user`, oldest first.
    fn releases(&self, user: &str, tid: &str) -> io::Result<Vec<ReleaseFact>>;
    /// Append one release; never updated.
    fn append_release(&self, user: &str, tid: &str, fact: &ReleaseFact) -> io::Result<()>;

    fn state_get(&self, key: &str) -> io::Result<Option<Vec<u8>>>;
    fn state_set(&self, key: &str, value: &[u8]) -> io::Result<()>;
    fn state_del(&self, key: &str) -> io::Result<()>;
    fn state_hget(&self, key: &str, field: &str) -> io::Result<Option<Vec<u8>>>;
    fn state_hset(&self, key: &str, field: &str, value: &[u8]) -> io::Result<()>;
    /// Every field of hash `key`, in no order.
    fn state_hgetall(&self, key: &str) -> io::Result<Vec<(String, Vec<u8>)>>;
    fn state_sadd(&self, key: &str, member: &str) -> io::Result<()>;
    fn state_smembers(&self, key: &str) -> io::Result<Vec<String>>;
}
