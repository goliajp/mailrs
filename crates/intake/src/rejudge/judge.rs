//! From one observation to the mail it concerns, judged again.

use mailrs_mailbox::ledger::LedgerVerdict;
use mailrs_reputation::{Kind, NeverList, Observation};

use super::facts;
use super::recall::{self, Candidate};
use super::store::{RawReader, RejudgeStore};
use super::{COUNTS, Counts};
use crate::fraud_judge::{Disposition, FraudJudge, FraudJudgement, disposition};

/// Messages judged in one page. An observation that names more is judged
/// over several, its place kept between them (see `super::tick`).
/// `MAILRS_REJUDGE_PAGE` overrides it.
const PAGE: usize = 20_000;

pub fn page_size() -> usize {
    std::env::var("MAILRS_REJUDGE_PAGE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or(PAGE)
}

/// The rules and the sets they read, kept across rounds.
pub struct Judge {
    fraud: FraudJudge,
    screening: mailrs_core_sidestate::families::screening::Live,
    own: Vec<String>,
    never: NeverList,
    authserv: Option<String>,
}

/// One message to judge: `(message id, its reader)`.
pub type Target = (String, String);

impl Judge {
    pub fn new<S: RejudgeStore>(store: &S) -> Self {
        let own: Vec<String> = store
            .local_domains()
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        Self {
            fraud: store.fraud_judge(),
            screening: mailrs_core_sidestate::families::screening::Live::new(own.clone()),
            never: NeverList::builtin(&own),
            own,
            authserv: store.authserv_id(),
        }
    }

    /// Rebuild the screening sets, so the new observations are in them.
    pub fn refresh(&self, conn: &mut kevy_client::Connection) {
        self.screening.refresh(conn, chrono::Utc::now().timestamp());
    }

    /// The mail `o` names, in a fixed order so a page can be resumed. An
    /// address or a network has no index in the ledger (one address is not
    /// one sender), and a host on the never-list never gets an automatic
    /// reputation change: `None` for those.
    pub fn targets<S: RejudgeStore>(&self, store: &S, o: &Observation) -> Option<Vec<Target>> {
        let value = o.value.trim().trim_end_matches('.').to_ascii_lowercase();
        let reg = mailrs_reputation::registrable(&value);
        let found = match o.kind {
            _ if self.never.contains(&value) => None,
            Kind::SenderDomain => Some(store.ledger_by_domain_tree(&value, &reg)),
            Kind::LinkDomain => Some(store.ledger_by_link_tree(&value, &reg)),
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
    pub fn page<S: RejudgeStore>(&mut self, store: &S, page: &[Target], obs: &str) -> u64 {
        let mut reader = store.raw_reader();
        let mut judged = 0;
        for (mid, user) in page {
            judged += u64::from(self.message(store, user, mid, &mut reader, obs));
        }
        judged
    }

    fn message<S: RejudgeStore>(
        &mut self,
        store: &S,
        user: &str,
        mid: &str,
        reader: &mut S::Reader<'_>,
        obs: &str,
    ) -> bool {
        let raw = reader.raw(user, mid);
        let Some(raw) = raw else {
            Counts::add(&COUNTS.no_file, 1);
            return false;
        };
        Counts::add(&COUNTS.messages, 1);
        let judged = match self.fraud.judge(user, &raw, &store.extracted(mid)) {
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
        let old = store.ledger_verdict(user, mid).ok().flatten();
        if old.as_ref().is_some_and(|o| !differs(o, &new)) {
            // a hold an earlier round left unheld is this round's to offer
            if new.holds && recall::unheld_by_another(store, user, mid, obs) {
                act(store, user, mid, &raw, &judged, &new, obs);
            }
            return true;
        }
        if let Err(e) = store.record_verdict(user, mid, &new) {
            tracing::warn!(error = %e, %user, %mid, "rejudge: the verdict was not recorded");
            return true;
        }
        Counts::add(&COUNTS.changed, 1);
        act(store, user, mid, &raw, &judged, &new, obs);
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
fn act<S: RejudgeStore>(
    store: &S,
    user: &str,
    mid: &str,
    raw: &[u8],
    judged: &FraudJudgement,
    new: &LedgerVerdict,
    obs: &str,
) {
    let Some((tid, row)) = store.thread_of(user, mid) else {
        return;
    };
    let in_junk = row.in_junk;
    let action = match disposition(judged.holds, judged.junks, row.held) {
        Disposition::Act if !row.held => {
            Counts::add(&COUNTS.would_hold, 1);
            "hold"
        }
        Disposition::Release if report_holds(store, user, &tid) => return,
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
        // with acting on, a later observation offers it again
        if action == "hold"
            && let Err(e) = recall::leave_unheld(store, user, mid, obs)
        {
            tracing::warn!(error = %e, %user, %mid, "rejudge: the unheld message was not marked");
        }
        tracing::info!(
            event = "rejudge_shadow", %user, %mid, observation = %obs, action,
            rules = ?new.rules, screening = ?new.screening,
            "rejudge: would act (shadow, nothing moved)"
        );
        return;
    }
    // a hold is gathered for the round's one decision (`recall::settle`)
    if action == "hold" {
        let candidate = Candidate {
            user: user.to_string(),
            tid,
            mid: mid.to_string(),
            from: mailrs_inbound::from_header(raw),
            subject: mailrs_inbound::subject_header(raw),
            rules: new.rules.clone(),
            verdict: judged.verdict.clone(),
            arrived: mailrs_inbound::dates::received_epoch(raw),
        };
        if let Err(e) = recall::offer(store, obs, &candidate) {
            tracing::warn!(error = %e, %user, tid = %candidate.tid,
                "rejudge: the conversation to hold was not kept for the round");
        }
        return;
    }
    let done = recall::unrecall(store, user, &tid, &judged.verdict.rules_version);
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

/// Whether a person's report holds the conversation, so no rule may let
/// it go. A hold list that cannot be read keeps it held.
fn report_holds<S: RejudgeStore>(store: &S, user: &str, tid: &str) -> bool {
    facts::held_by_report(store, user, tid).unwrap_or_else(|e| {
        tracing::warn!(error = %e, %user, %tid, "rejudge: the holds could not be read; left held");
        true
    })
}
