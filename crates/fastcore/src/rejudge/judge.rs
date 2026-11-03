//! From one observation to the mail it concerns, judged again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mailrs_mailbox_kevy::ledger::LedgerVerdict;
use mailrs_reputation::{Kind, NeverList, Observation};

use super::recall::{self, Recall};
use super::{COUNTS, Counts};
use crate::FastcoreState;
use crate::maintenance::fraud_rescan::{Disposition, FraudJudge, FraudJudgement, disposition};

/// Messages judged in one page. An observation that names more is judged
/// over several, its place kept between them (see `super::tick`).
/// `MAILRS_REJUDGE_PAGE` overrides it.
const PAGE: usize = 20_000;

pub(crate) fn page_size() -> usize {
    std::env::var("MAILRS_REJUDGE_PAGE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or(PAGE)
}

/// The rules and the sets they read, kept across rounds.
pub(crate) struct Judge {
    fraud: FraudJudge,
    screening: mailrs_core_sidestate::families::screening::Live,
    own: Vec<String>,
    never: NeverList,
    authserv: Option<String>,
}

/// One message to judge: `(message id, its reader)`.
pub(crate) type Target = (String, String);

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
            authserv: state.authserv_id.clone(),
        }
    }

    /// Rebuild the screening sets, so the new observations are in them.
    pub(crate) fn refresh(&self, conn: &mut kevy_client::Connection) {
        self.screening.refresh(conn, chrono::Utc::now().timestamp());
    }

    /// The mail `o` names, in a fixed order so a page can be resumed. An
    /// address or a network has no index in the ledger (one address is not
    /// one sender), and a host on the never-list never gets an automatic
    /// reputation change: `None` for those.
    pub(crate) fn targets(
        &self,
        state: &Arc<FastcoreState>,
        o: &Observation,
    ) -> Option<Vec<Target>> {
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
            return None;
        };
        let mut found = match found {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, id = %o.id, "rejudge: the ledger could not be read");
                return Some(Vec::new());
            }
        };
        found.sort();
        found.dedup();
        Some(found)
    }

    /// Judge one page of an observation's mail. Returns how many messages
    /// were judged.
    pub(crate) fn page(&mut self, state: &Arc<FastcoreState>, page: &[Target], obs: &str) -> u64 {
        let mut disks: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
        let mut judged = 0;
        for (mid, user) in page {
            let disk = disks
                .entry(user.clone())
                .or_insert_with(|| crate::maildir_scan::paths_on_disk(state, user));
            judged += u64::from(self.message(state, user, mid, disk, obs));
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
        let judged = match self.fraud.judge(state, user, mid, &raw) {
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
        act(state, user, mid, &raw, &judged, &new, obs);
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
            self.authserv.as_deref(),
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

/// Do what the new verdict calls for, or, while acting is off, log it.
/// Only a hold or the release of a conversation this module held is acted
/// on; Junk stays a logged suggestion (a score is the reader's to act on).
fn act(
    state: &Arc<FastcoreState>,
    user: &str,
    mid: &str,
    raw: &[u8],
    judged: &FraudJudgement,
    new: &LedgerVerdict,
    obs: &str,
) {
    let Some((tid, row)) = thread_of(state, user, mid) else {
        return;
    };
    let in_junk = mailrs_mailbox_kevy::scope::bucket_of(&row.category)
        == mailrs_mailbox_kevy::scope::Bucket::Junk;
    let action = match disposition(judged.holds, judged.junks, row.quarantined) {
        Disposition::Act if !row.quarantined => {
            Counts::add(&COUNTS.would_hold, 1);
            "hold"
        }
        Disposition::Release => {
            Counts::add(&COUNTS.would_release, 1);
            "release"
        }
        Disposition::Junk if !in_junk => {
            Counts::add(&COUNTS.would_junk, 1);
            "junk"
        }
        _ => return,
    };
    if !recall::acting() || action == "junk" {
        tracing::info!(
            event = "rejudge_shadow", %user, %mid, observation = %obs, action,
            rules = ?new.rules, screening = ?new.screening,
            "rejudge: would act (shadow, nothing moved)"
        );
        return;
    }
    let done = match action {
        "hold" => {
            let from = mailrs_inbound::from_header(raw);
            let subject = mailrs_inbound::subject_header(raw);
            recall::recall(
                state,
                &Recall {
                    user,
                    tid: &tid,
                    mid,
                    from: &from,
                    subject: &subject,
                    rules: new.rules.clone(),
                    observation: obs,
                    verdict: &judged.verdict,
                },
            )
        }
        _ => recall::unrecall(state, user, &tid),
    };
    match done {
        Ok(true) => {
            Counts::add(&COUNTS.acted, 1);
            tracing::info!(event = "rejudge_acted", %user, %tid, observation = %obs, action,
                rules = ?new.rules, "rejudge: acted");
        }
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, %user, %tid, action, "rejudge: acting failed"),
    }
}

/// The conversation `mid` is in, for `user`, and its row.
pub(crate) fn thread_of(
    state: &FastcoreState,
    user: &str,
    mid: &str,
) -> Option<(String, mailrs_mailbox_kevy::ThreadRow)> {
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
    Some((tid, row))
}
