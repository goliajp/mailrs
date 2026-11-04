//! Reporting a scam, for both cores: the reporter's copy is held at once,
//! the other copies of the same template from the last thirty days are
//! found, and they are taken back into Review when the report has a ground
//! and nothing says to wait, else listed for an administrator. When, is
//! `mailrs_report::decide`; where the mail is, is each core's
//! [`ReportStore`].

use std::io;

use kevy_client::Connection;
use mailrs_core_sidestate::families::{domain_first, reports};
use mailrs_report::{Decision, Situation, Template};

mod admin;
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
    /// The fraud rules in force hold `mid` (a rule in shadow is never in
    /// the stored verdict; screening rules are not fraud rules).
    fn held_by_rule(&self, mid: &str) -> bool;
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
        notify::held(&mut conn, user, &tid);
    }
    notify::audit(
        &mut conn,
        user,
        "report.made",
        &tid,
        &format!("report={} message={mid}", r.id),
    );
    let targets = others(core, user, mid, template.as_ref());
    let from = raw
        .as_deref()
        .map(mailrs_inbound::identity::from_address)
        .unwrap_or_default();
    let alike = alike_reports(core, &mut conn, &r, template.as_ref())?;
    let situation = Situation {
        reporter_is_administrator: administrator,
        reporters: alike.iter().map(|o| o.person.clone()).collect(),
        hold_rule: core.held_by_rule(mid),
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
    );
    reports::set_outcome(&mut conn, &r, &outcome).map_err(|e| e.to_string())?;
    if outcome.state == "taken_back" {
        settle_waiting(&mut conn, &r, &alike, &targets)?;
    }
    Ok(Some((r, outcome)))
}

/// Every other conversation holding a copy: the other readers of this very
/// message, and the readers of each message of the window that is the
/// same template.
fn others<S: ReportStore + ?Sized>(
    core: &S,
    reporter: &str,
    mid: &str,
    template: Option<&Template>,
) -> Vec<Target> {
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
    let mut out: Vec<Target> = Vec::new();
    for m in mids {
        for reader in core.readers(&m).unwrap_or_default() {
            if reader.eq_ignore_ascii_case(reporter) {
                continue;
            }
            let Ok(Some((tid, held))) = core.conversation_of(&reader, &m) else {
                continue;
            };
            let t = Target {
                user: reader,
                tid,
                mid: m.clone(),
            };
            // each message, not each conversation: a conversation a person
            // put back may still be taken for a copy that came after
            if !held && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out.sort_by(|a, b| (&a.user, &a.tid).cmp(&(&b.user, &b.tid)));
    out
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
) -> reports::Outcome {
    let mut o = reports::Outcome {
        inboxes: inboxes(targets) as u64,
        targets: targets.iter().map(Target::encode).collect(),
        ..Default::default()
    };
    match decision {
        Decision::NoneFound => {
            o.state = "none_found".into();
            o.decided_at = r.at;
        }
        Decision::TakeBack { grounds } => {
            o.grounds = names(grounds);
            o.taken_back = take_back(core, conn, r, targets, actor, &o.grounds);
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
    grounds: &[String],
) -> u64 {
    let batch = format!("report:{}", r.id);
    let mut taken = 0;
    for t in targets {
        match core.take_back(&t.user, &t.tid, &t.mid, &batch) {
            Ok(true) => {
                taken += 1;
                let raw = core.raw_of(&t.user, &t.mid).unwrap_or_default();
                notify::held(conn, &t.user, &t.tid);
                notify::taken(conn, t, &batch, &raw);
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, user = %t.user, tid = %t.tid,
                "report: a copy could not be taken back"),
        }
    }
    let detail = format!(
        "report={} fingerprint={} sender_domain={} taken_back={taken} of={} grounds={}",
        r.id,
        r.simhash,
        r.sender_domain,
        targets.len(),
        grounds.join(",")
    );
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
mod tests;
