//! Rules read from webapi's health answer: storage, accounts and what
//! webapi reads from kevy-server.

use super::{DEFAULT_REPEAT, Dep, Eval, Rule, Severity, Source, text};
use crate::readings::Snapshot;

const MIN: i64 = 60;
const GB: u64 = 1 << 30;

pub(super) static RULES: &[&Rule] = &[
    &KEVY_MEMORY,
    &NO_QUOTA,
    &QUOTA_80,
    &NEW_DOMAINS,
    &VOLUME,
    &FRAUD_HITS,
];

fn one(object: &str, e: Eval) -> Vec<(String, Eval)> {
    vec![(object.to_string(), e)]
}

/// A counted block of webapi health (`quota`, `new_domains`, `fraud_hits`),
/// unless it is stale: counts older than 15 minutes are not a reading of
/// now.
fn counted<'a>(s: &'a Snapshot, field: &str) -> Result<&'a serde_json::Value, Eval> {
    let v = s.webapi_field(field)?;
    match v["stale"].as_bool() {
        Some(false) => Ok(v),
        Some(true) => Err(Eval::field(format!("webapi {field} is stale"))),
        None => Err(Eval::field(format!("webapi {field} has no stale flag"))),
    }
}

fn number(v: &serde_json::Value, key: &str, field: &str) -> Result<u64, Eval> {
    v[key]
        .as_u64()
        .ok_or_else(|| Eval::field(format!("webapi {field}.{key} is not a number")))
}

pub static KEVY_MEMORY: Rule = Rule {
    name: "kevy_server_memory_high",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 10 * MIN,
    clear_secs: 10 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: Some(Dep::KevyServer),
    during_maintenance: false,
    object_kind: "process",
    text: &text::KEVY_MEMORY,
    eval: |s| {
        let e = s.webapi_field("kevy_server_memory").and_then(|m| {
            let used = number(m, "used_bytes", "kevy_server_memory")?;
            Ok(match m["limit_bytes"].as_u64() {
                None => Eval::clear("no limit configured"),
                Some(limit) if u128::from(used) * 100 >= u128::from(limit) * 85 => {
                    Eval::holds(format!("{used} of {limit} bytes"), ">= 85%")
                }
                Some(limit) => Eval::clear(format!("{used} of {limit} bytes")),
            })
        });
        one("kevy-server", e.unwrap_or_else(|e| e))
    },
};

pub static NO_QUOTA: Rule = Rule {
    name: "accounts_without_quota",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: Some(24 * 3600),
    depends: Some(Dep::KevyServer),
    during_maintenance: false,
    object_kind: "account",
    text: &text::NO_QUOTA,
    eval: |s| {
        let e = counted(s, "quota").and_then(|q| {
            number(q, "accounts", "quota")?;
            let n = number(q, "accounts_without_limit", "quota")?;
            Ok(match n {
                0 => Eval::clear(0),
                n => Eval::holds(n, "> 0"),
            })
        });
        one("accounts", e.unwrap_or_else(|e| e))
    },
};

pub static QUOTA_80: Rule = Rule {
    name: "accounts_over_80_percent",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 60 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: Some(Dep::KevyServer),
    during_maintenance: false,
    object_kind: "account",
    text: &text::QUOTA_80,
    eval: |s| {
        let e = counted(s, "quota").and_then(|q| {
            number(q, "accounts", "quota")?;
            let n = number(q, "over_80", "quota")?;
            Ok(match n {
                0 => Eval::clear(0),
                n => Eval::holds(n, ">= 1 account at 80%"),
            })
        });
        one("accounts", e.unwrap_or_else(|e| e))
    },
};

pub static NEW_DOMAINS: Rule = Rule {
    name: "new_domains_burst",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: DEFAULT_REPEAT,
    depends: Some(Dep::KevyServer),
    during_maintenance: false,
    object_kind: "domain",
    text: &text::NEW_DOMAINS,
    eval: |s| {
        let limit = s.limits.new_domains_per_day;
        let e = counted(s, "new_domains").and_then(|d| {
            // a census without its key count is not one
            number(d, "keys", "new_domains")?;
            let n = number(d, "added_24h", "new_domains")?;
            Ok(match n > limit {
                true => Eval::holds(n, format!("> {limit} in 24h")),
                false => Eval::clear(n),
            })
        });
        one("domains", e.unwrap_or_else(|e| e))
    },
};

pub static VOLUME: Rule = Rule {
    name: "data_volume_low",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 5 * MIN,
    clear_secs: 30 * MIN,
    repeat_secs: DEFAULT_REPEAT,
    depends: None,
    during_maintenance: true,
    object_kind: "volume",
    text: &text::VOLUME,
    eval: |s| {
        let e = s
            .webapi_field("jmap_uploads.volume_free_bytes")
            .and_then(|v| {
                v.as_u64()
                    .ok_or_else(|| Eval::field("volume_free_bytes is not a number"))
            })
            .map(|free| match free {
                f if f < 2 * GB => Eval::Holds {
                    value: format!("{f} bytes free"),
                    threshold: "< 2 GB".into(),
                    severity: Some(Severity::Critical),
                },
                f if f < 10 * GB => Eval::holds(format!("{f} bytes free"), "< 10 GB"),
                f => Eval::clear(format!("{f} bytes free")),
            });
        one("mailrs-data", e.unwrap_or_else(|e| e))
    },
};

/// Only whether the fraud rule counters can be read: the counts themselves
/// are a review feed, not a condition, so this rule never holds and its one
/// notice is the "cannot read" one after the unknown wait.
pub static FRAUD_HITS: Rule = Rule {
    name: "fraud_hits_unreadable",
    source: Source::Webapi,
    severity: Severity::Warning,
    for_secs: 0,
    clear_secs: 0,
    repeat_secs: None,
    depends: Some(Dep::KevyServer),
    during_maintenance: false,
    object_kind: "counter",
    text: &text::FRAUD_HITS,
    eval: |s| {
        let e = counted(s, "fraud_hits").and_then(|f| {
            let rules = f["by_rule_30d"]
                .as_object()
                .ok_or_else(|| Eval::field("webapi fraud_hits.by_rule_30d is not an object"))?;
            Ok(Eval::clear(format!("{} counters", rules.len())))
        });
        one("fraud_hits", e.unwrap_or_else(|e| e))
    },
};
