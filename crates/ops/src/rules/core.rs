//! Rules read from fastcore's own answers.

use mailrs_core_api::types::{BackendKind, HealthResponse};

use super::{DEFAULT_REPEAT, Eval, Rule, Severity, Source, text};
use crate::readings::Snapshot;

const MIN: i64 = 60;

pub(super) static RULES: &[&Rule] = &[
    &QUARANTINED,
    &CORE_MEMORY,
    &SPOOL_STALLED,
    &SET_ASIDE,
    &PARSING,
    &DAMAGED,
];

fn health(s: &Snapshot) -> Result<&HealthResponse, Eval> {
    s.fastcore
        .got()
        .ok_or_else(|| Eval::down(format!("fastcore: {}", s.fastcore.why())))
}

fn one(object: &str, e: Eval) -> Vec<(String, Eval)> {
    vec![(object.to_string(), e)]
}

pub static QUARANTINED: Rule = Rule {
    name: "spool_quarantined",
    source: Source::Fastcore,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "spool",
    text: &text::QUARANTINED,
    eval: |s| {
        let e = match health(s).map(|h| h.spool) {
            Err(e) => e,
            Ok(None) => Eval::field("fastcore reports no spool"),
            Ok(Some(sp)) if sp.quarantined > 0 => Eval::holds(sp.quarantined, "> 0"),
            Ok(Some(sp)) => Eval::clear(sp.quarantined),
        };
        one("spool", e)
    },
};

/// At or past `percent` of `limit`.
fn at_least(used: u64, limit: u64, percent: u64) -> bool {
    u128::from(used) * 100 >= u128::from(limit) * u128::from(percent)
}

pub static CORE_MEMORY: Rule = Rule {
    name: "core_memory_high",
    source: Source::Fastcore,
    severity: Severity::Warning,
    for_secs: 10 * MIN,
    clear_secs: 10 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "process",
    text: &text::CORE_MEMORY,
    eval: |s| {
        let e = match health(s).map(|h| h.memory) {
            Err(e) => e,
            // the cgroup could not be read: no reading, not "fine"
            Ok(None) => Eval::field("fastcore reports no memory"),
            Ok(Some(m)) => match m.limit_bytes {
                // read, and the container has no ceiling to near
                None => Eval::clear("no container limit"),
                Some(limit) if at_least(m.used_bytes, limit, 85) => {
                    Eval::holds(format!("{} of {limit} bytes", m.used_bytes), ">= 85%")
                }
                Some(limit) => Eval::clear(format!("{} of {limit} bytes", m.used_bytes)),
            },
        };
        one("fastcore", e)
    },
};

pub static SPOOL_STALLED: Rule = Rule {
    name: "spool_stalled",
    source: Source::Fastcore,
    severity: Severity::Critical,
    for_secs: 5 * MIN,
    clear_secs: 5 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "spool",
    text: &text::SPOOL_STALLED,
    eval: |s| {
        let e = match (health(s).map(|h| h.spool), s.fastcore_ops.got()) {
            (Err(e), _) => e,
            (Ok(None), _) => Eval::field("fastcore reports no spool"),
            (Ok(Some(_)), None) => Eval::field(format!("ops status: {}", s.fastcore_ops.why())),
            (Ok(Some(sp)), Some(_)) => match s.drain_still_secs {
                None => Eval::field("drain progress seen only once"),
                Some(still) if sp.oldest_incoming_age_seconds > 300 && still >= 300 => Eval::holds(
                    format!(
                        "oldest {}s, nothing delivered for {still}s",
                        sp.oldest_incoming_age_seconds
                    ),
                    "> 300s and 300s",
                ),
                Some(still) => Eval::clear(format!(
                    "oldest {}s, delivered {still}s ago",
                    sp.oldest_incoming_age_seconds
                )),
            },
        };
        one("spool", e)
    },
};

pub static SET_ASIDE: Rule = Rule {
    name: "work_set_aside",
    source: Source::Fastcore,
    severity: Severity::Warning,
    for_secs: 30 * MIN,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    // the sender's count is read from kevy-server through webapi
    depends: Some(super::Dep::KevyServer),
    during_maintenance: false,
    object_kind: "queue",
    text: &text::SET_ASIDE,
    eval: |s| {
        let count = |n: Option<u64>, what: &str| match n {
            None => Eval::field(format!("{what} count unreadable")),
            Some(0) => Eval::clear(0),
            Some(n) => Eval::holds(n, "> 0"),
        };
        let core = match health(s) {
            Err(e) => vec![
                ("attachment_text".to_string(), e.clone()),
                ("fraud_backfill".to_string(), e),
            ],
            Ok(h) => match h.set_aside {
                None => vec![
                    (
                        "attachment_text".to_string(),
                        Eval::field("fastcore reports no set_aside"),
                    ),
                    (
                        "fraud_backfill".to_string(),
                        Eval::field("fastcore reports no set_aside"),
                    ),
                ],
                Some(sa) => vec![
                    (
                        "attachment_text".to_string(),
                        count(sa.attachment_text, "attachment text"),
                    ),
                    // null when no backfill runs: nothing can have been set aside
                    (
                        "fraud_backfill".to_string(),
                        count(Some(sa.fraud_backfill.unwrap_or(0)), "fraud backfill"),
                    ),
                ],
            },
        };
        let sender = match s.webapi_field("set_aside.sender") {
            Err(e) => e,
            Ok(v) => count(v.as_u64(), "sender"),
        };
        let mut v = core;
        v.push(("sender".to_string(), sender));
        v
    },
};

fn parsing_eval(increase: Option<u64>, paused_since: Option<i64>, now: i64) -> Eval {
    let paused = paused_since.map(|t| now - t).filter(|secs| *secs > 300);
    match (increase, paused) {
        (_, Some(secs)) => Eval::holds(format!("a worker paused for {secs}s"), "> 300s"),
        (Some(n), None) if n >= 20 => {
            Eval::holds(format!("{n} failures in the last hour"), ">= 20")
        }
        (Some(n), None) => Eval::clear(n),
        (None, None) => Eval::field("failure count seen only once"),
    }
}

pub static PARSING: Rule = Rule {
    name: "parse_failures",
    source: Source::Fastcore,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 60 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "process",
    text: &text::PARSING,
    eval: |s| {
        let core = match health(s) {
            Err(e) => e,
            Ok(h) => match &h.parsing {
                None => Eval::field("fastcore reports no parsing"),
                Some(p) => parsing_eval(s.core_parse_increase, p.paused_since, s.now),
            },
        };
        let receiver = match s.fresh("receiver").and_then(|r| match &r.detail {
            Some(mailrs_ops_report::wire::Detail::Receiver(b)) => Some(b),
            _ => None,
        }) {
            None => Eval::down("no fresh receiver heartbeat"),
            Some(b) => match &b.parsing {
                None => Eval::field("receiver reports no parsing"),
                Some(p) => parsing_eval(s.receiver_parse_increase, p.paused_since, s.now),
            },
        };
        vec![("core".into(), core), ("receiver".into(), receiver)]
    },
};

pub static DAMAGED: Rule = Rule {
    name: "core_store_damaged",
    source: Source::Fastcore,
    severity: Severity::Critical,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: true,
    object_kind: "process",
    text: &text::DAMAGED,
    eval: |s| {
        // fastcore's own ready report says so even while its port is down
        let reported = s
            .processes
            .get("fastcore")
            .and_then(|p| match &p.latest.detail {
                Some(mailrs_ops_report::wire::Detail::CoreReady { boot_intact }) => {
                    Some(*boot_intact)
                }
                _ => None,
            });
        let probed = s
            .fastcore_ready
            .got()
            .filter(|h| h.backend == BackendKind::Kevy)
            .map(|h| h.ready);
        let e = match (probed, reported) {
            (Some(false), _) | (None, Some(false)) => Eval::holds("DAMAGED", "boot replay intact"),
            (Some(true), _) | (None, Some(true)) => Eval::clear("intact"),
            (None, None) => Eval::down(format!("fastcore readyz: {}", s.fastcore_ready.why())),
        };
        one("fastcore", e)
    },
};
