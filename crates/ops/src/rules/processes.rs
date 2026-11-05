//! Rules read from the reports processes push.

use std::collections::BTreeSet;

use mailrs_ops_report::wire::Detail;

use super::{DEFAULT_REPEAT, Eval, Rule, Severity, Source, text};

const MIN: i64 = 60;

pub(super) static RULES: &[&Rule] = &[&CRASH_LOOP, &VERSIONS, &OUTBOUND_STUCK];

pub static CRASH_LOOP: Rule = Rule {
    name: "crash_loop",
    source: Source::Reports,
    severity: Severity::Critical,
    for_secs: 0,
    clear_secs: 15 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "process",
    text: &text::CRASH_LOOP,
    eval: |s| {
        let mut out = Vec::new();
        for (name, p) in &s.processes {
            let limit = match name.as_str() {
                "fastcore" => s.limits.fastcore_ready_secs,
                _ => s.limits.ready_secs,
            };
            for i in &p.instances {
                let object = format!("{name}/{}", i.instance);
                let e = match (i.starts_15m, i.waiting_ready_secs) {
                    (n, _) if n >= 3 => Eval::holds(format!("{n} starts in 15 minutes"), ">= 3"),
                    (_, Some(w)) if w > limit => Eval::holds(
                        format!("started {w}s ago, not ready"),
                        format!("ready within {limit}s"),
                    ),
                    (n, _) => Eval::clear(format!("{n} starts in 15 minutes")),
                };
                out.push((object, e));
            }
        }
        out
    },
};

pub static VERSIONS: Rule = Rule {
    name: "versions_differ",
    source: Source::Reports,
    severity: Severity::Critical,
    for_secs: 5 * MIN,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "deployment",
    text: &text::VERSIONS,
    eval: |s| {
        let label = |version: &str, commit: &Option<String>| match commit {
            Some(c) => format!("{version}@{}", &c[..c.len().min(12)]),
            None => version.to_string(),
        };
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        if let Some(b) = s.fastcore.got().and_then(|h| h.build.as_ref()) {
            seen.insert((label(&b.version, &b.commit), "fastcore".into()));
        }
        if let Some(v) = s.webapi.got().and_then(|h| h["build"]["version"].as_str()) {
            let commit = s
                .webapi
                .got()
                .and_then(|h| h["build"]["commit"].as_str())
                .map(String::from);
            seen.insert((label(v, &commit), "webapi".into()));
        }
        for (name, p) in &s.processes {
            if p.fresh {
                seen.insert((
                    label(&p.latest.build.version, &p.latest.build.commit),
                    name.clone(),
                ));
            }
        }
        let builds: BTreeSet<&String> = seen.iter().map(|(b, _)| b).collect();
        let listing = seen
            .iter()
            .map(|(b, p)| format!("{p}={b}"))
            .collect::<Vec<_>>()
            .join(", ");
        let e = match builds.len() {
            0 => Eval::down("no process reported its build"),
            1 => Eval::clear(listing),
            _ => Eval::holds(listing, "one build"),
        };
        vec![("deployment".into(), e)]
    },
};

pub static OUTBOUND_STUCK: Rule = Rule {
    name: "outbound_stuck",
    source: Source::Sender,
    severity: Severity::Critical,
    for_secs: 5 * MIN,
    clear_secs: 10 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: false,
    object_kind: "process",
    text: &text::OUTBOUND_STUCK,
    eval: |s| {
        let beat = s.fresh("sender").and_then(|r| match &r.detail {
            Some(Detail::Sender(b)) => Some(b),
            _ => None,
        });
        let e = match beat {
            None => Eval::down("no fresh sender heartbeat"),
            Some(b) => match (b.pending, b.oldest_pending_age_secs) {
                (None, _) => Eval::field("sender could not read its queue"),
                (Some(n), Some(age)) if n > 0 && age > 3600 => {
                    Eval::holds(format!("{n} waiting, oldest {age}s"), "oldest <= 3600s")
                }
                (Some(n), _) if n > 0 && b.last_pop_at.is_none_or(|t| s.now - t > 600) => {
                    Eval::holds(
                        format!(
                            "{n} waiting, last take {:?}",
                            b.last_pop_at.map(|t| s.now - t)
                        ),
                        "a take within 600s",
                    )
                }
                (Some(n), _) => Eval::clear(format!("{n} waiting")),
            },
        };
        vec![("sender".into(), e)]
    },
};
