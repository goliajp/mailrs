//! The kevy store as re-judging sees it (`mailrs_intake::rejudge`): the
//! ledger, the conversations and their hold facts, and the re-judge's own
//! records, all in the embedded store under the keys they always had.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_intake::fraud_judge::{Extracted, FraudJudge};
use mailrs_intake::rejudge::{Conversation, RawReader, RejudgeStore};
use mailrs_mailbox::ledger::LedgerVerdict;
use mailrs_mailbox::scope::{Bucket, bucket_of};

use crate::FastcoreState;

fn text(v: Vec<u8>) -> String {
    String::from_utf8_lossy(&v).into_owned()
}

/// One page's files, a reader at a time: one scan of a reader's maildir
/// serves every message of theirs on the page.
pub struct Reader<'a> {
    state: &'a FastcoreState,
    disks: HashMap<String, HashMap<String, PathBuf>>,
}

impl RawReader for Reader<'_> {
    fn raw(&mut self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let state = self.state;
        let disk = self
            .disks
            .entry(user.to_string())
            .or_insert_with(|| crate::maildir_scan::paths_on_disk(state, user));
        let copy = state.mailbox.user_message_facts(user, mid).ok().flatten()?;
        let path = disk.get(crate::maildir_scan::base_id(&copy.blob_ref))?;
        std::fs::read(path).ok()
    }
}

fn conversation(row: &mailrs_mailbox_kevy::ThreadRow) -> Conversation {
    Conversation {
        held: row.quarantined,
        in_junk: bucket_of(&row.category) == Bucket::Junk,
    }
}

impl RejudgeStore for FastcoreState {
    type Reader<'a> = Reader<'a>;

    fn raw_reader(&self) -> Reader<'_> {
        Reader {
            state: self,
            disks: HashMap::new(),
        }
    }

    fn local_domains(&self) -> Vec<String> {
        crate::spool_drain::relay::local_domains(self).to_vec()
    }

    fn authserv_id(&self) -> Option<String> {
        self.authserv_id.clone()
    }

    fn fraud_judge(&self) -> FraudJudge {
        crate::maintenance::fraud_rescan::fraud_judge(self)
    }

    fn extracted(&self, mid: &str) -> Extracted {
        crate::maintenance::fraud_rescan::extracted(self, mid)
    }

    fn ledger_by_domain_tree(&self, domain: &str, reg: &str) -> io::Result<Vec<(String, String)>> {
        self.mailbox.ledger_by_domain_tree(domain, reg)
    }

    fn ledger_by_link_tree(&self, host: &str, reg: &str) -> io::Result<Vec<(String, String)>> {
        self.mailbox.ledger_by_link_tree(host, reg)
    }

    fn ledger_verdict(&self, user: &str, mid: &str) -> io::Result<Option<LedgerVerdict>> {
        self.mailbox.ledger_verdict(user, mid)
    }

    fn record_verdict(&self, user: &str, mid: &str, v: &LedgerVerdict) -> io::Result<()> {
        self.mailbox.record_verdict(user, mid, v)
    }

    fn thread_of(&self, user: &str, mid: &str) -> Option<(String, Conversation)> {
        let tid = self
            .mailbox
            .user_message_view(user, mid)
            .ok()
            .flatten()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["thread_id"].as_str().map(str::to_string))?;
        let row = self
            .mailbox
            .get_thread_for_user(user, &tid)
            .ok()
            .flatten()?;
        Some((tid, conversation(&row)))
    }

    fn conversation(&self, user: &str, tid: &str) -> io::Result<Option<Conversation>> {
        Ok(self
            .mailbox
            .get_thread_for_user(user, tid)?
            .map(|row| conversation(&row)))
    }

    fn thread_message_ids(&self, user: &str, tid: &str) -> io::Result<Vec<String>> {
        self.mailbox.user_thread_message_ids(user, tid)
    }

    fn fraud_verdict(&self, mid: &str) -> io::Result<Option<String>> {
        self.mailbox.fraud_verdict(mid)
    }

    fn set_fraud_verdict(&self, mid: &str, json: &str) -> io::Result<()> {
        self.mailbox.set_fraud_verdict(mid, json)
    }

    fn hold(&self, user: &str, tid: &str, source: &str, rules_version: &str) -> io::Result<bool> {
        if !self.mailbox.set_quarantined(user, tid, true)? {
            return Ok(false);
        }
        crate::hold_facts::record_hold_fact(self, user, tid, source, rules_version)
            .map_err(io::Error::other)?;
        Ok(true)
    }

    fn release(&self, user: &str, tid: &str) -> io::Result<bool> {
        self.mailbox.set_quarantined(user, tid, false)
    }

    fn last_hold_source(&self, user: &str, tid: &str) -> io::Result<Option<String>> {
        Ok(crate::hold_facts::holds(self, user, tid)
            .map_err(io::Error::other)?
            .pop()
            .map(|h| h.source))
    }

    fn releases(&self, user: &str, tid: &str) -> io::Result<Vec<ReleaseFact>> {
        crate::hold_facts::releases(self, user, tid).map_err(io::Error::other)
    }

    fn append_release(&self, user: &str, tid: &str, fact: &ReleaseFact) -> io::Result<()> {
        crate::hold_facts::append_release(self, user, tid, fact).map_err(io::Error::other)
    }

    fn state_get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.mailbox.store_ref().get(key.as_bytes())?)
    }

    fn state_set(&self, key: &str, value: &[u8]) -> io::Result<()> {
        self.mailbox.store_ref().set(key.as_bytes(), value)?;
        Ok(())
    }

    fn state_del(&self, key: &str) -> io::Result<()> {
        self.mailbox.store_ref().del(&[key.as_bytes()])?;
        Ok(())
    }

    fn state_hget(&self, key: &str, field: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self
            .mailbox
            .store_ref()
            .hget(key.as_bytes(), field.as_bytes())?)
    }

    fn state_hset(&self, key: &str, field: &str, value: &[u8]) -> io::Result<()> {
        self.mailbox
            .store_ref()
            .hset(key.as_bytes(), &[(field.as_bytes(), value)])?;
        Ok(())
    }

    fn state_hgetall(&self, key: &str) -> io::Result<Vec<(String, Vec<u8>)>> {
        Ok(self
            .mailbox
            .store_ref()
            .hgetall(key.as_bytes())?
            .into_iter()
            .map(|(f, v)| (text(f), v))
            .collect())
    }

    fn state_sadd(&self, key: &str, member: &str) -> io::Result<()> {
        self.mailbox
            .store_ref()
            .sadd(key.as_bytes(), &[member.as_bytes()])?;
        Ok(())
    }

    fn state_smembers(&self, key: &str) -> io::Result<Vec<String>> {
        Ok(self
            .mailbox
            .store_ref()
            .smembers(key.as_bytes())?
            .into_iter()
            .map(text)
            .collect())
    }
}
