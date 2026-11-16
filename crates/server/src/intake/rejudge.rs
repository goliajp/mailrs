//! This core as re-judging sees it (`mailrs_intake::rejudge`): its ledger,
//! conversations, hold and release facts, and the re-judge's own records
//! in `rejudge_kv` and `rejudge_sets`. Blocks on the database: the re-judge
//! runs on a thread of its own, its routes on a blocking task.

use std::io;

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_intake::fraud_judge::{Extracted, FraudJudge};
use mailrs_intake::rejudge::{Conversation, RawReader, RejudgeStore};
use mailrs_intake::route::Directory;
use mailrs_mailbox::ledger::LedgerVerdict;

use super::PgCore;

fn io(e: sqlx::Error) -> io::Error {
    io::Error::other(e)
}

impl mailrs_core_sidestate::NetKevy for PgCore {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(self.dir.net_url.as_deref()?).ok()
    }
}

/// Reads each message from its file, as the report reads it.
pub struct Reader<'a>(&'a PgCore);

impl RawReader for Reader<'_> {
    fn raw(&mut self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let core = self.0;
        let (_, file, _) = core
            .dir
            .rt
            .block_on(core.mailbox.message_conversation(user, mid))
            .ok()
            .flatten()?;
        core.dir.rt.block_on(crate::message_util::read_message_raw(
            &core.dir.maildir_root,
            user,
            &file,
        ))
    }
}

impl RejudgeStore for PgCore {
    type Reader<'a> = Reader<'a>;

    fn raw_reader(&self) -> Reader<'_> {
        Reader(self)
    }

    fn local_domains(&self) -> Vec<String> {
        Directory::local_domains(self).to_vec()
    }

    fn authserv_id(&self) -> Option<String> {
        self.dir.authserv_id.clone()
    }

    /// The account display names from this core's accounts; none when
    /// they cannot be read, which only makes a rule less likely to hold.
    fn fraud_judge(&self) -> FraudJudge {
        let names = match self.dir.rt.block_on(self.dir.domain.list_accounts()) {
            Ok(accounts) => accounts
                .into_iter()
                .map(|a| a.display_name.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "rejudge: the account names were not read");
                Vec::new()
            }
        };
        FraudJudge::new(
            mailrs_intake::scam_report::hold_rule::policy(names),
            self.dir.authserv_id.clone(),
            mailrs_core_sidestate::NetKevy::net_conn(self),
        )
    }

    /// This core reads no images for QR codes.
    fn extracted(&self, mid: &str) -> Extracted {
        let texts = self
            .dir
            .rt
            .block_on(self.mailbox.attachment_texts_of(mid))
            .unwrap_or_else(|e| {
                tracing::warn!(%mid, error = %e, "rejudge: the attachment text was not read");
                Vec::new()
            });
        Extracted {
            texts,
            qr_urls: Vec::new(),
        }
    }

    fn ledger_by_domain_tree(&self, domain: &str, reg: &str) -> io::Result<Vec<(String, String)>> {
        self.dir
            .rt
            .block_on(self.mailbox.ledger_by_domain_tree(domain, reg))
            .map_err(io)
    }

    fn ledger_by_link_tree(&self, host: &str, reg: &str) -> io::Result<Vec<(String, String)>> {
        self.dir
            .rt
            .block_on(self.mailbox.ledger_by_link_tree(host, reg))
            .map_err(io)
    }

    fn ledger_verdict(&self, user: &str, mid: &str) -> io::Result<Option<LedgerVerdict>> {
        self.dir
            .rt
            .block_on(self.mailbox.ledger_verdict(user, mid))
            .map_err(io)
    }

    fn record_verdict(&self, user: &str, mid: &str, v: &LedgerVerdict) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.record_verdict(user, mid, v))
            .map_err(io)
    }

    fn thread_of(&self, user: &str, mid: &str) -> Option<(String, Conversation)> {
        let (tid, _, _) = self
            .dir
            .rt
            .block_on(self.mailbox.message_conversation(user, mid))
            .ok()
            .flatten()?;
        let row = self.conversation(user, &tid).ok().flatten()?;
        Some((tid, row))
    }

    fn conversation(&self, user: &str, tid: &str) -> io::Result<Option<Conversation>> {
        let found = self
            .dir
            .rt
            .block_on(self.mailbox.rejudge_conversation(user, tid))
            .map_err(io)?;
        Ok(found.map(|(held, in_junk)| Conversation { held, in_junk }))
    }

    fn thread_message_ids(&self, user: &str, tid: &str) -> io::Result<Vec<String>> {
        self.dir
            .rt
            .block_on(self.mailbox.conversation_message_ids(user, tid))
            .map_err(io)
    }

    fn fraud_verdict(&self, mid: &str) -> io::Result<Option<String>> {
        let v = self
            .dir
            .rt
            .block_on(self.mailbox.fraud_verdict(mid))
            .map_err(io)?;
        Ok(v.map(|v| v.to_string()))
    }

    fn set_fraud_verdict(&self, mid: &str, json: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.replace_fraud_verdict(mid, json))
            .map_err(io)
    }

    /// Only a conversation the reader has: holding one they do not would
    /// leave a hold row for nothing.
    fn hold(&self, user: &str, tid: &str, source: &str, rules_version: &str) -> io::Result<bool> {
        if self.conversation(user, tid)?.is_none() {
            return Ok(false);
        }
        self.dir
            .rt
            .block_on(self.mailbox.hold_thread(user, tid, source, rules_version))
            .map_err(io)?;
        Ok(true)
    }

    fn release(&self, user: &str, tid: &str) -> io::Result<bool> {
        if self.conversation(user, tid)?.is_none() {
            return Ok(false);
        }
        self.dir
            .rt
            .block_on(self.mailbox.release_thread(user, tid))
            .map_err(io)?;
        Ok(true)
    }

    fn last_hold_source(&self, user: &str, tid: &str) -> io::Result<Option<String>> {
        let holds = self
            .dir
            .rt
            .block_on(self.mailbox.holds(user, tid))
            .map_err(io)?;
        Ok(holds.into_iter().last().map(|h| h.source))
    }

    fn releases(&self, user: &str, tid: &str) -> io::Result<Vec<ReleaseFact>> {
        let rows = self
            .dir
            .rt
            .block_on(self.mailbox.releases(user, tid))
            .map_err(io)?;
        Ok(rows
            .into_iter()
            .map(|r| ReleaseFact {
                at: r.at,
                source: r.source,
                actor: r.actor,
                rules_version: r.rules_version,
            })
            .collect())
    }

    fn append_release(&self, user: &str, tid: &str, fact: &ReleaseFact) -> io::Result<()> {
        let row = mailrs_mailbox::pg::ReleaseFact {
            at: fact.at,
            source: fact.source.clone(),
            actor: fact.actor.clone(),
            rules_version: fact.rules_version.clone(),
        };
        self.dir
            .rt
            .block_on(self.mailbox.record_release(user, tid, &row))
            .map_err(io)
    }

    fn state_get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        self.state_hget(key, "")
    }

    fn state_set(&self, key: &str, value: &[u8]) -> io::Result<()> {
        self.state_hset(key, "", value)
    }

    fn state_del(&self, key: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_delete(key))
            .map_err(io)
    }

    fn state_hget(&self, key: &str, field: &str) -> io::Result<Option<Vec<u8>>> {
        let v = self
            .dir
            .rt
            .block_on(self.mailbox.rejudge_get(key, field))
            .map_err(io)?;
        Ok(v.map(String::into_bytes))
    }

    fn state_hset(&self, key: &str, field: &str, value: &[u8]) -> io::Result<()> {
        let value = String::from_utf8_lossy(value);
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_put(key, field, &value))
            .map_err(io)
    }

    fn state_hgetall(&self, key: &str) -> io::Result<Vec<(String, Vec<u8>)>> {
        let rows = self
            .dir
            .rt
            .block_on(self.mailbox.rejudge_fields(key))
            .map_err(io)?;
        Ok(rows.into_iter().map(|(f, v)| (f, v.into_bytes())).collect())
    }

    fn state_hdel(&self, key: &str, field: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_delete_field(key, field))
            .map_err(io)
    }

    fn state_sadd(&self, key: &str, member: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_add_member(key, member))
            .map_err(io)
    }

    fn state_smembers(&self, key: &str) -> io::Result<Vec<String>> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_members(key))
            .map_err(io)
    }

    fn state_srem(&self, key: &str, member: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_remove_member(key, member))
            .map_err(io)
    }

    fn state_keys(&self, prefix: &str) -> io::Result<Vec<String>> {
        self.dir
            .rt
            .block_on(self.mailbox.rejudge_keys(prefix))
            .map_err(io)
    }
}
