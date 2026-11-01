//! From one observation to the mail it concerns, judged again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mailrs_mailbox_kevy::ledger::LedgerVerdict;
use mailrs_reputation::{Kind, NeverList, Observation};

use super::{COUNTS, Counts};
use crate::FastcoreState;
use crate::maintenance::fraud_rescan::{Disposition, FraudJudge, disposition};

/// Messages judged for one observation at most. Past it the rest wait
/// for the acting stage, which will page; until then the count says how
/// often an observation reached that far.
const CAP: usize = 20_000;

/// The rules and the sets they read, kept across rounds.
pub(crate) struct Judge {
    fraud: FraudJudge,
    screening: mailrs_core_sidestate::families::screening::Live,
    own: Vec<String>,
    never: NeverList,
}

impl Judge {
    pub(crate) fn new(state: &Arc<FastcoreState>) -> Self {
        let own: Vec<String> = crate::spool_drain::relay::local_domains(state)
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        Self {
            fraud: FraudJudge::new(state),
            screening: mailrs_core_sidestate::families::screening::Live::new(own.clone()),
            never: NeverList::builtin(&own),
            own,
        }
    }

    /// Rebuild the screening sets, so the new observations are in them.
    pub(crate) fn refresh(&self, conn: &mut kevy_client::Connection) {
        self.screening.refresh(conn, chrono::Utc::now().timestamp());
    }

    /// Judge again the mail `o` names. An address or a network has no
    /// index in the ledger (one address is not one sender), and a host on
    /// the never-list never gets an automatic reputation change. Returns
    /// how many messages were judged.
    pub(crate) fn observation(&mut self, state: &Arc<FastcoreState>, o: &Observation) -> u64 {
        let value = o.value.trim().trim_end_matches('.').to_ascii_lowercase();
        let reg = mailrs_reputation::registrable(&value);
        let found = match o.kind {
            _ if self.never.contains(&value) => None,
            Kind::SenderDomain => Some(state.mailbox.ledger_by_domain_tree(&value, &reg)),
            Kind::LinkDomain => Some(state.mailbox.ledger_by_link_tree(&value, &reg)),
            Kind::Ip | Kind::Cidr => None,
        };
        let Some(found) = found else {
            Counts::add(&COUNTS.skipped, 1);
            return 0;
        };
        let mut found = match found {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, id = %o.id, "rejudge: the ledger could not be read");
                return 0;
            }
        };
        if found.len() > CAP {
            tracing::warn!(id = %o.id, found = found.len(), "rejudge: more mail than one round judges");
            Counts::add(&COUNTS.over_cap, 1);
            found.truncate(CAP);
        }
        let mut disks: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
        let mut judged = 0;
        for (mid, user) in found {
            let disk = disks
                .entry(user.clone())
                .or_insert_with(|| crate::maildir_scan::paths_on_disk(state, &user));
            judged += u64::from(self.message(state, &user, &mid, disk, &o.id));
        }
        judged
    }

    fn message(
        &mut self,
        state: &Arc<FastcoreState>,
        user: &str,
        mid: &str,
        disk: &HashMap<String, PathBuf>,
        obs: &str,
    ) -> bool {
        let raw = state
            .mailbox
            .user_message_facts(user, mid)
            .ok()
            .flatten()
            .and_then(|copy| {
                disk.get(crate::maildir_scan::base_id(&copy.blob_ref))
                    .cloned()
            })
            .and_then(|path| std::fs::read(path).ok());
        let Some(raw) = raw else {
            Counts::add(&COUNTS.no_file, 1);
            return false;
        };
        Counts::add(&COUNTS.messages, 1);
        let judged = match self.fraud.judge(state, mid, &raw) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, %user, %mid, "rejudge: the fraud rules could not run");
                return false;
            }
        };
        let Some(screening) = self.screen(&raw) else {
            return false;
        };
        let new = crate::ledger::judged_verdict(&judged.findings, &judged.version, screening);
        let old = state.mailbox.ledger_verdict(user, mid).ok().flatten();
        if old.as_ref().is_some_and(|o| !differs(o, &new)) {
            return true;
        }
        if let Err(e) = state.mailbox.record_verdict(user, mid, &new) {
            tracing::warn!(error = %e, %user, %mid, "rejudge: the verdict was not recorded");
            return true;
        }
        Counts::add(&COUNTS.changed, 1);
        let Some(action) = would_do(state, user, mid, new.holds, judged.junks) else {
            return true;
        };
        tracing::info!(
            event = "rejudge_shadow", %user, %mid, observation = %obs, action,
            rules = ?new.rules, screening = ?new.screening,
            "rejudge: would act (shadow, nothing moved)"
        );
        true
    }

    /// What the screening rules match in `raw`, in the mode they run in.
    fn screen(&self, raw: &[u8]) -> Option<Vec<String>> {
        let envelope_domain = mailrs_fraud_lua::stored::header_value(raw, b"return-path")
            .and_then(|v| {
                v.trim()
                    .trim_matches(['<', '>'])
                    .rsplit_once('@')
                    .map(|(_, d)| d.to_ascii_lowercase())
            })
            .unwrap_or_default();
        let verdict = mailrs_screening_lua::stored::with_input(
            &self.screening.reputation(),
            &self.screening.inspector(),
            &self.own,
            raw,
            &envelope_domain,
            mailrs_screening_lua::evaluate,
        );
        match verdict {
            Ok(v) => Some(v.hits.into_iter().map(|h| h.rule).collect()),
            Err(e) => {
                self.screening.failed("rules", &e);
                None
            }
        }
    }
}

/// The rules say something different: a hold either way, another set of
/// rules, another screening match. A score that moved alone is not a
/// different verdict.
fn differs(old: &LedgerVerdict, new: &LedgerVerdict) -> bool {
    old.holds != new.holds || old.rules != new.rules || old.screening != new.screening
}

/// What acting on the verdict would change, counted; `None` when it would
/// change nothing.
fn would_do(
    state: &FastcoreState,
    user: &str,
    mid: &str,
    holds: bool,
    junks: bool,
) -> Option<&'static str> {
    let tid = state
        .mailbox
        .user_message_view(user, mid)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["thread_id"].as_str().map(str::to_string))?;
    let row = state
        .mailbox
        .get_thread_for_user(user, &tid)
        .ok()
        .flatten()?;
    let in_junk = mailrs_mailbox_kevy::scope::bucket_of(&row.category)
        == mailrs_mailbox_kevy::scope::Bucket::Junk;
    match disposition(holds, junks, row.quarantined) {
        Disposition::Act if !row.quarantined => {
            Counts::add(&COUNTS.would_hold, 1);
            Some("hold")
        }
        Disposition::Release => {
            Counts::add(&COUNTS.would_release, 1);
            Some("release")
        }
        Disposition::Junk if !in_junk => {
            Counts::add(&COUNTS.would_junk, 1);
            Some("junk")
        }
        _ => None,
    }
}
