//! This core's half of reporting a scam: where its fingerprints, readers
//! and conversations are. The report itself is `mailrs_intake::scam_report`,
//! shared with the SQL core.

use std::io;
use std::sync::Arc;

use mailrs_intake::scam_report::ReportStore;
use mailrs_report::Template;

use crate::FastcoreState;

/// Fingerprint candidates read per piece.
const CANDIDATES: usize = 2_000;

/// Records a delivered message's fingerprint once per message. Logged on
/// failure: delivery does not need it.
pub(crate) fn record(state: &FastcoreState, user: &str, mid: &str, raw: &[u8], arrived: i64) {
    match state.mailbox.template_of(mid) {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(%mid, error = %e, "report: the fingerprint could not be read");
            return;
        }
    }
    let Some(t) = mailrs_intake::scam_report::template::template_of(user, raw) else {
        return;
    };
    if let Err(e) = state.mailbox.record_template(mid, &t, arrived) {
        tracing::warn!(%mid, error = %e, "report: the fingerprint was not recorded");
    }
}

/// The kevy core as a report sees it.
pub(crate) struct Reports<'a>(pub(crate) &'a Arc<FastcoreState>);

impl ReportStore for Reports<'_> {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        self.0.net_conn()
    }

    fn conversation_of(&self, user: &str, mid: &str) -> io::Result<Option<(String, bool)>> {
        Ok(crate::rejudge::judge::thread_of(self.0, user, mid)
            .map(|(tid, row)| (tid, row.quarantined)))
    }

    fn raw_of(&self, user: &str, mid: &str) -> Option<Vec<u8>> {
        let copy = self
            .0
            .mailbox
            .user_message_facts(user, mid)
            .ok()
            .flatten()?;
        let (local, domain) = user.split_once('@')?;
        let root = std::path::Path::new(&self.0.maildir_root)
            .join(domain)
            .join(local);
        let path = mailrs_maildir::find(&root, &copy.blob_ref).ok().flatten()?;
        std::fs::read(path).ok()
    }

    fn stored_template(&self, mid: &str) -> io::Result<Option<Template>> {
        Ok(self.0.mailbox.template_of(mid)?.map(|t| t.template))
    }

    fn record_template(&self, mid: &str, t: &Template, arrived: i64) -> io::Result<bool> {
        self.0.mailbox.record_template(mid, t, arrived)
    }

    fn candidates(&self, simhash: u64, since: i64) -> io::Result<Vec<String>> {
        self.0
            .mailbox
            .template_candidates(simhash, since, CANDIDATES)
    }

    fn readers(&self, mid: &str) -> io::Result<Vec<String>> {
        self.0.mailbox.ledger_readers(mid)
    }

    fn held_by_rule(&self, mid: &str) -> bool {
        self.0
            .mailbox
            .fraud_verdict(mid)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str::<mailrs_inbound::FraudVerdict>(&j).ok())
            .is_some_and(|v| v.quarantined)
    }

    fn hold(&self, user: &str, tid: &str) -> io::Result<()> {
        match self.0.mailbox.set_quarantined(user, tid, true)? {
            true => Ok(()),
            false => Err(io::Error::other("thread membership disappeared")),
        }
    }

    fn take_back(&self, user: &str, tid: &str, batch: &str) -> Result<bool, String> {
        crate::rejudge::recall::hold_reported(self.0, user, tid, batch)
    }
}
