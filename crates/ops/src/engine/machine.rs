//! The state machine of one alert (one rule, one object).
//!
//! | from | to | when |
//! |---|---|---|
//! | ok | pending | the condition holds |
//! | pending | ok | it stops holding within `for`; no notice |
//! | pending | firing | it held for `for`; notice |
//! | firing | recovering | it stops holding |
//! | recovering | firing | it holds again within `clear`; no notice |
//! | recovering | ok | it stayed clear for `clear`; "resolved" |
//! | any | unknown | three readings in a row could not be read |
//! | unknown | the phase before | a reading comes back; the gap does not count toward `for` |
//!
//! A `for` or `clear` of zero moves at once. Time is counted only between
//! two readable evaluations, so a stretch of unreadable cycles never fills
//! a `for` window.

use serde::{Deserialize, Serialize};

use crate::rules::{Eval, Severity};

/// Readings in a row that cannot be read before the alert is unknown.
pub const MISSES_TO_UNKNOWN: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Ok,
    Pending,
    Firing,
    Recovering,
    Unknown,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Ok => "ok",
            Phase::Pending => "pending",
            Phase::Firing => "firing",
            Phase::Recovering => "recovering",
            Phase::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        match s {
            "ok" => Some(Phase::Ok),
            "pending" => Some(Phase::Pending),
            "firing" => Some(Phase::Firing),
            "recovering" => Some(Phase::Recovering),
            "unknown" => Some(Phase::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Firing,
    Repeat,
    /// Still firing, now at a higher severity.
    Escalated,
    Resolved,
    /// Could not be read for the unknown wait.
    Unreadable,
    /// Readable again after an `Unreadable` notice.
    Readable,
}

impl NoticeKind {
    pub const ALL: [NoticeKind; 6] = [
        NoticeKind::Firing,
        NoticeKind::Repeat,
        NoticeKind::Escalated,
        NoticeKind::Resolved,
        NoticeKind::Unreadable,
        NoticeKind::Readable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NoticeKind::Firing => "firing",
            NoticeKind::Repeat => "repeat",
            NoticeKind::Escalated => "escalated",
            NoticeKind::Resolved => "resolved",
            NoticeKind::Unreadable => "unreadable",
            NoticeKind::Readable => "readable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertState {
    pub phase: Phase,
    /// When the current phase was entered.
    pub since: i64,
    pub before_unknown: Option<Phase>,
    /// Seconds counted toward `for` (pending) or `clear` (recovering).
    pub progress: i64,
    pub last_readable_at: Option<i64>,
    pub misses: u32,
    pub first_miss_at: Option<i64>,
    pub unreadable_noticed: bool,
    pub firing_since: Option<i64>,
    pub notified_at: Option<i64>,
    pub severity: Option<Severity>,
    pub value: Option<String>,
    pub threshold: Option<String>,
    pub why: Option<String>,
}

impl AlertState {
    pub fn new(now: i64) -> Self {
        AlertState {
            phase: Phase::Ok,
            since: now,
            before_unknown: None,
            progress: 0,
            last_readable_at: None,
            misses: 0,
            first_miss_at: None,
            unreadable_noticed: false,
            firing_since: None,
            notified_at: None,
            severity: None,
            value: None,
            threshold: None,
            why: None,
        }
    }

    /// A phase rebuilt from the last recorded transition, timers fresh.
    pub fn rebuilt(phase: Phase, since: i64) -> Self {
        let mut s = AlertState::new(since);
        match phase {
            Phase::Unknown => {
                s.phase = Phase::Unknown;
                s.before_unknown = Some(Phase::Ok);
            }
            p => s.phase = p,
        }
        if matches!(phase, Phase::Firing | Phase::Recovering) {
            s.firing_since = Some(since);
            s.notified_at = Some(since);
        }
        s
    }

    /// Pending, firing or recovering: the alert is in play.
    pub fn active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Pending | Phase::Firing | Phase::Recovering
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub for_secs: i64,
    pub clear_secs: i64,
    pub unknown_wait: i64,
    pub base: Severity,
}

/// What one evaluation did.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    /// `(from, to, reason)` for every phase change, in order.
    pub transitions: Vec<(Phase, Phase, &'static str)>,
    pub notices: Vec<NoticeKind>,
}

impl Step {
    fn go(&mut self, st: &mut AlertState, to: Phase, now: i64, reason: &'static str) {
        self.transitions.push((st.phase, to, reason));
        st.phase = to;
        st.since = now;
        st.progress = 0;
    }
}

/// Advance `st` by one evaluation. `repeat` is the resolved repeat interval
/// for the severity in effect, `None` for a rule notified once per firing.
pub fn step(
    st: &mut AlertState,
    eval: &Eval,
    now: i64,
    t: &Timing,
    repeat: impl Fn(Severity) -> Option<i64>,
) -> Step {
    let mut out = Step::default();
    let (holds, value, threshold, severity) = match eval {
        Eval::Unreadable { why, source_down } => {
            unreadable(st, &mut out, now, t, why, *source_down);
            return out;
        }
        Eval::Holds {
            value,
            threshold,
            severity,
        } => (
            true,
            value,
            Some(threshold),
            severity.unwrap_or(t.base).max(t.base),
        ),
        Eval::Clear { value } => (false, value, None, t.base),
    };
    if st.phase == Phase::Unknown {
        let back = st.before_unknown.take().unwrap_or(Phase::Ok);
        let progress = st.progress;
        out.go(st, back, now, "readable again");
        st.progress = progress;
        if st.unreadable_noticed {
            out.notices.push(NoticeKind::Readable);
        }
    }
    st.misses = 0;
    st.first_miss_at = None;
    st.unreadable_noticed = false;
    st.why = None;
    st.value = Some(value.clone());
    if let Some(th) = threshold {
        st.threshold = Some(th.clone());
    }
    let dt = st.last_readable_at.map_or(0, |at| now - at);
    st.last_readable_at = Some(now);
    match (st.phase, holds) {
        (Phase::Ok, true) => {
            out.go(st, Phase::Pending, now, "condition holds");
            if t.for_secs <= 0 {
                fire(st, &mut out, now, severity);
            }
        }
        (Phase::Pending, true) => {
            st.progress += dt;
            if st.progress >= t.for_secs {
                fire(st, &mut out, now, severity);
            }
        }
        (Phase::Pending, false) => out.go(st, Phase::Ok, now, "cleared within for"),
        (Phase::Firing, true) => {
            let notified = st.notified_at.unwrap_or(now);
            if st.severity.is_some_and(|s| severity > s) {
                st.severity = Some(severity);
                st.notified_at = Some(now);
                out.notices.push(NoticeKind::Escalated);
            } else if repeat(severity).is_some_and(|r| now - notified >= r) {
                st.notified_at = Some(now);
                out.notices.push(NoticeKind::Repeat);
            }
        }
        (Phase::Firing, false) => {
            out.go(st, Phase::Recovering, now, "condition cleared");
            if t.clear_secs <= 0 {
                resolve(st, &mut out, now);
            }
        }
        (Phase::Recovering, true) => {
            out.go(st, Phase::Firing, now, "held again within clear");
        }
        (Phase::Recovering, false) => {
            st.progress += dt;
            if st.progress >= t.clear_secs {
                resolve(st, &mut out, now);
            }
        }
        (Phase::Ok, false) => {}
        (Phase::Unknown, _) => unreachable!("left unknown above"),
    }
    out
}

fn fire(st: &mut AlertState, out: &mut Step, now: i64, severity: Severity) {
    out.go(st, Phase::Firing, now, "held for the for window");
    st.firing_since = Some(now);
    st.notified_at = Some(now);
    st.severity = Some(severity);
    out.notices.push(NoticeKind::Firing);
}

fn resolve(st: &mut AlertState, out: &mut Step, now: i64) {
    out.go(st, Phase::Ok, now, "clear for the clear window");
    st.firing_since = None;
    st.notified_at = None;
    st.severity = None;
    out.notices.push(NoticeKind::Resolved);
}

fn unreadable(
    st: &mut AlertState,
    out: &mut Step,
    now: i64,
    t: &Timing,
    why: &str,
    source_down: bool,
) {
    st.misses += 1;
    let first = *st.first_miss_at.get_or_insert(now);
    st.why = Some(why.to_string());
    st.last_readable_at = None;
    if st.phase != Phase::Unknown && st.misses >= MISSES_TO_UNKNOWN {
        st.before_unknown = Some(st.phase);
        let progress = st.progress;
        out.go(st, Phase::Unknown, now, "unreadable three times");
        // kept for the phase it returns to; only the gap is not counted
        st.progress = progress;
    }
    // a source that is down has its own notice; one per rule would repeat it
    if st.phase == Phase::Unknown
        && !source_down
        && !st.unreadable_noticed
        && now - first >= t.unknown_wait
    {
        st.unreadable_noticed = true;
        out.notices.push(NoticeKind::Unreadable);
    }
}

#[cfg(test)]
mod tests;
