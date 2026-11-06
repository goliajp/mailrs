//! Rules about the daemon itself, and the rules that stand for a whole
//! source or a dependency.

use super::{DEFAULT_REPEAT, Dep, Eval, Rule, Severity, Source, Text, text};
use crate::readings::Snapshot;

/// `for_secs` of a rule that waits the configured unknown wait.
pub const UNKNOWN_WAIT: i64 = -1;

pub(super) static RULES: &[&Rule] = &[
    &CHANNEL_FAILING,
    &UNWATCHED,
    &MAINTENANCE_LONG,
    &KEVY_DOWN,
    &NO_REPORTS,
    &SRC_FASTCORE,
    &SRC_WEBAPI,
    &SRC_RECEIVER,
    &SRC_SENDER,
    &SRC_FETCH,
    &SRC_BACKUP,
];

pub static CHANNEL_FAILING: Rule = Rule {
    name: super::CHANNEL_FAILING,
    source: Source::Daemon,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "channel",
    text: &text::CHANNEL,
    eval: |s| {
        s.channels
            .iter()
            .map(|c| {
                let e = match c.consecutive_failures {
                    n if n >= 3 => Eval::holds(format!("{n} failures in a row"), "< 3"),
                    n => Eval::clear(format!("{n} failures in a row")),
                };
                (c.name.clone(), e)
            })
            .collect()
    },
};

pub static UNWATCHED: Rule = Rule {
    name: "daemon_unwatched",
    source: Source::Daemon,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: None,
    depends: None,
    during_maintenance: false,
    object_kind: "daemon",
    text: &text::UNWATCHED,
    eval: |s| {
        let e = match s.heartbeat_configured {
            false => Eval::holds("MAILRS_OPS_HEARTBEAT_URL unset", "a heartbeat URL"),
            true => Eval::clear("heartbeat configured"),
        };
        vec![("ops".into(), e)]
    },
};

pub static MAINTENANCE_LONG: Rule = Rule {
    name: "maintenance_too_long",
    source: Source::Daemon,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: true,
    object_kind: "maintenance",
    text: &text::MAINTENANCE_LONG,
    eval: |s| {
        let limit = s.limits.maintenance_alert_secs;
        let e = match &s.maintenance {
            Some(m) if s.now - m.started_at > limit => Eval::holds(
                format!("open {}s by {} ({})", s.now - m.started_at, m.by, m.reason),
                format!("<= {limit}s"),
            ),
            Some(m) => Eval::clear(format!("open {}s", s.now - m.started_at)),
            None => Eval::clear("off"),
        };
        vec![("maintenance".into(), e)]
    },
};

pub static KEVY_DOWN: Rule = Rule {
    name: super::KEVY_SERVER_UNAVAILABLE,
    source: Source::Daemon,
    severity: Severity::Critical,
    for_secs: 120,
    clear_secs: 120,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "process",
    text: &text::KEVY_DOWN,
    eval: |s| {
        let e = match &s.kevy {
            crate::readings::Reading::Got(false) => {
                Eval::holds("unreachable on every path", "reachable")
            }
            crate::readings::Reading::Got(true) => Eval::clear("reachable"),
            crate::readings::Reading::Unreadable(why) => Eval::field(why),
        };
        vec![("kevy-server".into(), e)]
    },
};

pub static NO_REPORTS: Rule = Rule {
    name: super::NO_REPORTS,
    source: Source::Daemon,
    severity: Severity::Warning,
    for_secs: UNKNOWN_WAIT,
    clear_secs: 0,
    repeat_secs: None,
    depends: None,
    during_maintenance: false,
    object_kind: "daemon",
    text: &text::NO_REPORTS,
    eval: |s| {
        let heard = s
            .push_sources
            .iter()
            .filter(|p| s.fresh(p).is_some())
            .count();
        let e = match (s.push_sources.len(), heard) {
            (0, _) => Eval::clear("no process is expected to push"),
            (n, 0) => Eval::holds(format!("0 of {n} pushing processes heard"), "at least one"),
            (n, h) => Eval::clear(format!("{h} of {n} heard")),
        };
        vec![("reports".into(), e)]
    },
};

fn pulled(source: Source, s: &Snapshot) -> Vec<(String, Eval)> {
    let why = match source {
        Source::Fastcore => s.fastcore.got().map_or(Some(s.fastcore.why()), |_| None),
        _ => s.webapi.got().map_or(Some(s.webapi.why()), |_| None),
    };
    let e = match why {
        Some(w) => Eval::holds(format!("cannot read: {w}"), "readable"),
        None => Eval::clear("readable"),
    };
    vec![(source.as_str().to_string(), e)]
}

fn pushed(source: Source, s: &Snapshot) -> Vec<(String, Eval)> {
    let name = source.as_str();
    if !s.push_sources.iter().any(|p| p == name) {
        return Vec::new();
    }
    let e = match s.processes.get(name) {
        Some(p) if p.fresh => Eval::clear("heartbeat fresh"),
        Some(p) => Eval::holds(
            format!("last report {}s ago", p.age_secs),
            "a fresh heartbeat",
        ),
        None => Eval::holds("never reported", "a fresh heartbeat"),
    };
    vec![(name.to_string(), e)]
}

macro_rules! source_rule {
    ($id:ident, $name:literal, $src:expr, $eval:ident, $dep:expr) => {
        pub static $id: Rule = Rule {
            name: concat!("source_unreadable.", $name),
            source: $src,
            severity: Severity::Warning,
            for_secs: UNKNOWN_WAIT,
            clear_secs: 0,
            repeat_secs: None,
            depends: $dep,
            during_maintenance: false,
            object_kind: "process",
            text: &text::SOURCE,
            eval: |s| $eval($src, s),
        };
    };
}

source_rule!(SRC_FASTCORE, "fastcore", Source::Fastcore, pulled, None);
source_rule!(SRC_WEBAPI, "webapi", Source::Webapi, pulled, None);
source_rule!(
    SRC_RECEIVER,
    "receiver",
    Source::Receiver,
    pushed,
    Some(Dep::ReportsArriving)
);
source_rule!(
    SRC_SENDER,
    "sender",
    Source::Sender,
    pushed,
    Some(Dep::ReportsArriving)
);
source_rule!(
    SRC_FETCH,
    "fetch",
    Source::Fetch,
    pushed,
    Some(Dep::ReportsArriving)
);
source_rule!(
    SRC_BACKUP,
    "backup",
    Source::Backup,
    pushed,
    Some(Dep::ReportsArriving)
);

/// The rules standing for each source.
pub fn source_rules() -> [&'static Rule; 6] {
    [
        &SRC_FASTCORE,
        &SRC_WEBAPI,
        &SRC_RECEIVER,
        &SRC_SENDER,
        &SRC_FETCH,
        &SRC_BACKUP,
    ]
}

/// The text of a field-level "cannot read".
pub fn source_text() -> &'static Text {
    &text::FIELD
}
