//! This core's half of reporting a scam: where its fingerprints, readers
//! and conversations are. The report itself is `mailrs_intake::scam_report`,
//! shared with the kevy core.

use std::io;

use mailrs_intake::scam_report::ReportStore;
use mailrs_mailbox::pg::StoredTemplate;
use mailrs_report::Template;

use super::PgCore;

/// Fingerprint candidates read per report.
const CANDIDATES: i64 = 8_000;

fn io(e: sqlx::Error) -> io::Error {
    io::Error::other(e)
}

impl PgCore {
    /// Records a delivered message's fingerprint once per message. Logged
    /// on failure: delivery does not need it.
    pub(super) fn record_template(&self, addr: &str, message_id: &str, body: &[u8]) {
        let Some(t) = mailrs_intake::scam_report::template::template_of(addr, body) else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        let arrived = mailrs_inbound::dates::received_epoch(body).unwrap_or(now);
        if let Err(e) = <Self as ReportStore>::record_template(self, message_id, &t, arrived) {
            tracing::warn!(%message_id, error = %e, "report: the fingerprint was not recorded");
        }
    }
}

impl ReportStore for PgCore {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(self.dir.net_url.as_deref()?).ok()
    }

    fn conversation_of(&self, user: &str, mid: &str) -> io::Result<Option<(String, bool)>> {
        let found = self
            .dir
            .rt
            .block_on(self.mailbox.message_conversation(user, mid))
            .map_err(io)?;
        Ok(found.map(|(tid, _, held)| (tid, held)))
    }

    fn raw_of(&self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let (_, file, _) = self
            .dir
            .rt
            .block_on(self.mailbox.message_conversation(user, mid))
            .ok()
            .flatten()?;
        self.dir.rt.block_on(crate::message_util::read_message_raw(
            &self.dir.maildir_root,
            user,
            &file,
        ))
    }

    fn stored_template(&self, mid: &str) -> io::Result<Option<Template>> {
        let t = self
            .dir
            .rt
            .block_on(self.mailbox.template_of(mid))
            .map_err(io)?;
        Ok(t.map(|t| Template {
            simhash: t.simhash,
            link_domains: t.link_domains,
            attachments: t.attachments,
            sender_domain: t.sender_domain,
        }))
    }

    fn record_template(&self, mid: &str, t: &Template, arrived: i64) -> io::Result<bool> {
        let stored = StoredTemplate {
            simhash: t.simhash,
            arrived,
            link_domains: t.link_domains.clone(),
            attachments: t.attachments.clone(),
            sender_domain: t.sender_domain.clone(),
        };
        let now = chrono::Utc::now().timestamp();
        self.dir
            .rt
            .block_on(self.mailbox.record_template(mid, &stored, now))
            .map_err(io)
    }

    fn candidates(&self, simhash: u64, since: i64) -> io::Result<Vec<String>> {
        self.dir
            .rt
            .block_on(self.mailbox.template_candidates(simhash, since, CANDIDATES))
            .map_err(io)
    }

    fn readers(&self, mid: &str) -> io::Result<Vec<String>> {
        self.dir
            .rt
            .block_on(self.mailbox.message_readers(mid))
            .map_err(io)
    }

    fn held_by_rule(&self, mid: &str) -> bool {
        self.dir
            .rt
            .block_on(self.mailbox.fraud_verdict(mid))
            .ok()
            .flatten()
            .and_then(|v| v.get("quarantined").and_then(serde_json::Value::as_bool))
            .unwrap_or(false)
    }

    fn hold(&self, user: &str, tid: &str) -> io::Result<()> {
        self.dir
            .rt
            .block_on(self.mailbox.hold_thread(user, tid))
            .map_err(io)
    }

    /// This core keeps no batches (`/v1/admin/rejudge-batches` is empty
    /// here): a conversation a report took back is released one by one.
    fn take_back(&self, user: &str, tid: &str, _batch: &str) -> Result<bool, String> {
        self.dir
            .rt
            .block_on(self.mailbox.hold_thread_if_free(user, tid))
            .map_err(|e| e.to_string())
    }
}
