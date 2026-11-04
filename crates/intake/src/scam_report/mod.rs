//! Reporting a scam, for both cores: the reporter's copy is held at once,
//! the other copies of the same template from the last thirty days are
//! found, and they are taken back into Review when the report has a ground
//! and nothing says to wait, else listed for an administrator. When, is
//! `mailrs_report::decide`; where the mail is, is each core's
//! [`ReportStore`].

use std::io;

use kevy_client::Connection;
use mailrs_core_sidestate::families::{domain_first, fraud_hits, reports};
use mailrs_report::{Decision, Situation, Template};

mod admin;
pub mod hold_rule;
mod notify;
pub mod template;

pub use admin::{Pending, confirm, dismiss, outcome_for, pending};

/// How far back a report looks for other copies, in days.
pub const WINDOW_DAYS: i64 = 30;

/// Where a core keeps what a report needs.
pub trait ReportStore {
    fn net_conn(&self) -> Option<Connection>;
    /// The conversation `mid` is in for `user`, and whether it is held;
    /// `None` when `user` has no such message.
    fn conversation_of(&self, user: &str, mid: &str) -> io::Result<Option<(String, bool)>>;
    /// `user`'s file for `mid`, while it is on disk.
    fn raw_of(&self, user: &str, mid: &str) -> Option<Vec<u8>>;
    fn stored_template(&self, mid: &str) -> io::Result<Option<Template>>;
    /// Records `mid`'s fingerprint once; true when written.
    fn record_template(&self, mid: &str, t: &Template, arrived: i64) -> io::Result<bool>;
    /// Messages arrived at `since` or later sharing a piece with `simhash`.
    fn candidates(&self, simhash: u64, since: i64) -> io::Result<Vec<String>>;
    /// The accounts holding a copy of `mid`.
    fn readers(&self, mid: &str) -> io::Result<Vec<String>>;
    /// What judging `mid` again reads besides its file: the policy (with
    /// this core's account names), the `authserv-id`, and the text of its
    /// attachments.
    fn fraud_inputs(&self, mid: &str) -> hold_rule::Inputs;
    /// The fraud rules in force on `facts`: the receiver's own entry, the
    /// bundle in `MAILRS_FRAUD_RULES_FILE`. Screening rules are not fraud
    /// rules.
    fn scan(
        &self,
        facts: &mailrs_fraud::Facts<'_>,
        policy: &mailrs_fraud::Policy,
    ) -> Result<mailrs_fraud_lua::Scan, String> {
        mailrs_fraud_lua::scan(facts, policy)
    }
    /// Hold the reporter's own conversation.
    fn hold(&self, user: &str, tid: &str) -> io::Result<()>;
    /// Take another reader's conversation back into Review for message
    /// `mid`, as part of `batch`, for an administrator to put back whole.
    /// False when it was already held, or when a person released the
    /// conversation after `mid` arrived: what a person put back stays put
    /// back, and only mail that came later may take it again.
    fn take_back(&self, user: &str, tid: &str, mid: &str, batch: &str) -> Result<bool, String>;
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// One other copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub user: String,
    pub tid: String,
    pub mid: String,
}

impl Target {
    fn encode(&self) -> String {
        format!("{}\t{}\t{}", self.user, self.tid, self.mid)
    }
    fn decode(s: &str) -> Option<Self> {
        let mut it = s.splitn(3, '\t');
        Some(Self {
            user: it.next()?.to_string(),
            tid: it.next()?.to_string(),
            mid: it.next()?.to_string(),
        })
    }
}

/// The stored fingerprint of `mid`, else one computed now from `user`'s
/// file and recorded.
pub fn fingerprint<S: ReportStore + ?Sized>(
    core: &S,
    user: &str,
    mid: &str,
    raw: Option<&[u8]>,
) -> Option<Template> {
    if let Ok(Some(t)) = core.stored_template(mid) {
        return Some(t);
    }
    let raw = raw?;
    let t = template::template_of(user, raw)?;
    let arrived = mailrs_inbound::dates::received_epoch(raw).unwrap_or_else(now);
    if let Err(e) = core.record_template(mid, &t, arrived) {
        tracing::warn!(%mid, error = %e, "report: the fingerprint was not recorded");
    }
    Some(t)
}

/// `user` reports `mid`. `None` when `user` has no such message.
pub fn report<S: ReportStore + ?Sized>(
    core: &S,
    user: &str,
    mid: &str,
    administrator: bool,
) -> Result<Option<(reports::Report, reports::Outcome)>, String> {
    let Some((tid, held)) = core.conversation_of(user, mid).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mut conn = core.net_conn().ok_or("the shared store is unreachable")?;
    let raw = core.raw_of(user, mid);
    let template = fingerprint(core, user, mid, raw.as_deref());
    let at = now();
    let hex = |t: &Template| format!("{:016x}", t.simhash);
    let r = reports::Report {
        id: reports::new_id(),
        reporter: user.to_ascii_lowercase(),
        person: reports::person_of(&mut conn, user).map_err(|e| e.to_string())?,
        message_id: mid.to_string(),
        thread_id: tid.clone(),
        at,
        simhash: template.as_ref().map(hex).unwrap_or_default(),
        sender_domain: template
            .as_ref()
            .map(|t| t.sender_domain.clone())
            .unwrap_or_default(),
    };
    reports::append(&mut conn, &r).map_err(|e| e.to_string())?;
    if !held {
        core.hold(user, &tid).map_err(|e| e.to_string())?;
        notify::held(&mut conn, user, user, &tid);
    }
    // a body too short to compare cannot find its copies: counted, so how
    // often that happens can be seen (`/api/health` → `fraud_hits`)
    if template.is_none()
        && let Err(e) = fraud_hits::record(&mut conn, &[reports::NO_FINGERPRINT.to_string()], at)
    {
        tracing::warn!(error = %e, "report: the no-fingerprint count was not kept");
    }
    notify::audit(
        &mut conn,
        user,
        "report.made",
        &tid,
        &format!("report={} message={mid}", r.id),
    );
    let (own, targets) = others(core, user, &tid, mid, template.as_ref());
    let own_held = hold_own(core, &mut conn, &r, &own);
    let from = raw
        .as_deref()
        .map(mailrs_inbound::identity::from_address)
        .unwrap_or_default();
    let alike = alike_reports(core, &mut conn, &r, template.as_ref())?;
    let rule = hold_rule::in_force(core, user, mid, raw.as_deref());
    if let Some(h) = &rule {
        notify::audit(
            &mut conn,
            user,
            "report.hold_rule",
            &tid,
            &format!(
                "report={} message={mid} rule={} rules_version={}",
                r.id, h.rule, h.version
            ),
        );
    }
    let situation = Situation {
        reporter_is_administrator: administrator,
        reporters: alike.iter().map(|o| o.person.clone()).collect(),
        hold_rule: rule.is_some(),
        inboxes: inboxes(&targets),
        sender_history_days: domain_first::first_seen(&mut conn, &r.sender_domain)
            .map(|first| (at - first) / 86_400),
        allow_listed: !from.is_empty()
            && reports::allow_listed(&mut conn, &from).map_err(|e| e.to_string())?,
    };
    let outcome = act(
        core,
        &mut conn,
        &r,
        &mailrs_report::decide(&situation),
        &targets,
        user,
        rule.as_ref(),
    );
    let outcome = reports::Outcome {
        own_held,
        no_fingerprint: template.is_none(),
        ..outcome
    };
    reports::set_outcome(&mut conn, &r, &outcome).map_err(|e| e.to_string())?;
    if outcome.state == "taken_back" {
        settle_waiting(&mut conn, &r, &alike, &targets)?;
    }
    Ok(Some((r, outcome)))
}

/// Every other conversation holding a copy: the other readers of this very
/// message, and the readers of each message of the window that is the
/// same template. The reporter's own other conversations come first,
/// apart: they are held with the report, whatever the grounds.
fn others<S: ReportStore + ?Sized>(
    core: &S,
    reporter: &str,
    reported_tid: &str,
    mid: &str,
    template: Option<&Template>,
) -> (Vec<Target>, Vec<Target>) {
    let mut mids = vec![mid.to_string()];
    if let Some(t) = template {
        let since = now() - WINDOW_DAYS * 86_400;
        for m in core.candidates(t.simhash, since).unwrap_or_default() {
            let same = core
                .stored_template(&m)
                .ok()
                .flatten()
                .is_some_and(|c| mailrs_report::same(t, &c).is_some());
            if same && !mids.contains(&m) {
                mids.push(m);
            }
        }
    }
    let (mut own, mut out): (Vec<Target>, Vec<Target>) = (Vec::new(), Vec::new());
    for m in mids {
        for reader in core.readers(&m).unwrap_or_default() {
            let Ok(Some((tid, held))) = core.conversation_of(&reader, &m) else {
                continue;
            };
            let mine = reader.eq_ignore_ascii_case(reporter);
            if held || mine && tid == reported_tid {
                continue;
            }
            let t = Target {
                user: reader,
                tid,
                mid: m.clone(),
            };
            // each message, not each conversation: a conversation a person
            // put back may still be taken for a copy that came after
            let into = match mine {
                true => &mut own,
                false => &mut out,
            };
            if !into.contains(&t) {
                into.push(t);
            }
        }
    }
    for list in [&mut own, &mut out] {
        list.sort_by(|a, b| (&a.user, &a.tid).cmp(&(&b.user, &b.tid)));
    }
    (own, out)
}

/// Hold the reporter's other copies of the template with the report, as
/// part of its batch: no ground is needed for what is the reporter's own.
/// A conversation the reporter put back before stays put back for a copy
/// that came before that, as every hold does. How many were held.
fn hold_own<S: ReportStore + ?Sized>(
    core: &S,
    conn: &mut Connection,
    r: &reports::Report,
    own: &[Target],
) -> u64 {
    let batch = format!("report:{}", r.id);
    let mut held = 0;
    for t in own {
        match core.take_back(&t.user, &t.tid, &t.mid, &batch) {
            Ok(true) => {
                held += 1;
                notify::held(conn, &r.reporter, &t.user, &t.tid);
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, tid = %t.tid,
                "report: one of the reporter's own copies could not be held"),
        }
    }
    held
}

fn inboxes(targets: &[Target]) -> usize {
    let mut users: Vec<&str> = targets.iter().map(|t| t.user.as_str()).collect();
    users.sort_unstable();
    users.dedup();
    users.len()
}

/// The reports of this template within the day, this one included: of the
/// same message, or of a message that is the same template.
fn alike_reports<S: ReportStore + ?Sized>(
    core: &S,
    conn: &mut Connection,
    r: &reports::Report,
    template: Option<&Template>,
) -> Result<Vec<reports::Report>, String> {
    let since = r.at - mailrs_report::decide::REPORT_WINDOW_SECS;
    let recent = reports::since(conn, since).map_err(|e| e.to_string())?;
    Ok(recent
        .into_iter()
        .filter(|other| {
            other.message_id == r.message_id
                || template.is_some_and(|t| {
                    core.stored_template(&other.message_id)
                        .ok()
                        .flatten()
                        .is_some_and(|o| mailrs_report::same(t, &o).is_some())
                })
        })
        .collect())
}

fn names<T: serde::Serialize>(v: &[T]) -> Vec<String> {
    v.iter()
        .filter_map(|x| serde_json::to_value(x).ok())
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

fn act<S: ReportStore + ?Sized>(
    core: &S,
    conn: &mut Connection,
    r: &reports::Report,
    decision: &Decision,
    targets: &[Target],
    actor: &str,
    rule: Option<&hold_rule::HoldRule>,
) -> reports::Outcome {
    let mut o = reports::Outcome {
        inboxes: inboxes(targets) as u64,
        targets: targets.iter().map(Target::encode).collect(),
        hold_rule: rule.map(|h| h.rule.clone()),
        rules_version: rule.map(|h| h.version.clone()),
        ..Default::default()
    };
    match decision {
        Decision::NoneFound => {
            o.state = "none_found".into();
            o.decided_at = r.at;
        }
        Decision::TakeBack { grounds } => {
            o.grounds = names(grounds);
            o.taken_back = take_back(core, conn, r, targets, actor, &o);
            o.state = "taken_back".into();
            o.decided_at = now();
        }
        Decision::Confirm { grounds, waits } => {
            o.grounds = names(grounds);
            o.waits = names(waits);
            o.state = "waiting".into();
        }
    }
    o
}

/// Take each target back into Review in this report's batch, tell its
/// reader, and audit the whole once; how many were taken.
fn take_back<S: ReportStore + ?Sized>(
    core: &S,
    conn: &mut Connection,
    r: &reports::Report,
    targets: &[Target],
    actor: &str,
    o: &reports::Outcome,
) -> u64 {
    let batch = format!("report:{}", r.id);
    let mut taken = 0;
    for t in targets {
        match core.take_back(&t.user, &t.tid, &t.mid, &batch) {
            Ok(true) => {
                taken += 1;
                let raw = core.raw_of(&t.user, &t.mid).unwrap_or_default();
                notify::held(conn, actor, &t.user, &t.tid);
                notify::taken(conn, t, &batch, &raw);
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, user = %t.user, tid = %t.tid,
                "report: a copy could not be taken back"),
        }
    }
    let mut detail = format!(
        "report={} fingerprint={} sender_domain={} taken_back={taken} of={} grounds={}",
        r.id,
        r.simhash,
        r.sender_domain,
        targets.len(),
        o.grounds.join(",")
    );
    if let (Some(rule), Some(version)) = (&o.hold_rule, &o.rules_version) {
        detail.push_str(&format!(" rule={rule} rules_version={version}"));
    }
    notify::audit(conn, actor, "report.take_back", &r.thread_id, &detail);
    taken
}

/// A report that took the template back settles the earlier reports of it
/// still waiting: what they waited for is what was just taken.
fn settle_waiting(
    conn: &mut Connection,
    r: &reports::Report,
    alike: &[reports::Report],
    taken: &[Target],
) -> Result<(), String> {
    let taken: Vec<String> = taken.iter().map(Target::encode).collect();
    let waiting = reports::pending(conn).map_err(|e| e.to_string())?;
    for earlier in alike
        .iter()
        .filter(|e| e.id != r.id && waiting.contains(&e.id))
    {
        let Some(mut o) = reports::outcome(conn, &earlier.id).map_err(|e| e.to_string())? else {
            continue;
        };
        o.state = "taken_back".into();
        o.taken_back = o.targets.iter().filter(|t| taken.contains(t)).count() as u64;
        o.decided_at = now();
        o.grounds.push(format!("report:{}", r.id));
        reports::set_outcome(conn, earlier, &o).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod hold_rule_tests;
#[cfg(test)]
mod tests;
