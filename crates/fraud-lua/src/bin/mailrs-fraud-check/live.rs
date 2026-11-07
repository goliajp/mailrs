//! The gate's first number: what is stored now against the candidate.
//!
//! `compare` judges the baseline and the candidate with this one binary,
//! so it isolates the rule change and cannot see what a new binary
//! changes: the stored verdicts were made by whichever binary ran when
//! each message arrived. The core's dry re-judge with the candidate
//! (`maintenance:fraud-rescan?candidate=<sha256>`) compares each
//! conversation's stored state with where the re-judge would put it; its
//! pages, one JSON object per line, are summed here and set beside the
//! pack-against-pack report.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

const COUNTS: &[&str] = &[
    "would_hold",
    "would_release",
    "would_move_to_junk",
    "would_move_out_of_junk",
    "would_move_to_inbox",
    "would_move_out_of_inbox",
];
const MAPS: &[&str] = &["would_hold_from", "would_release_to"];
const WALK: &[&str] = &[
    "threads_walked",
    "no_file",
    "verdict_failed",
    "kept_released",
    "held_but_unreadable",
    "scored_but_whitelisted",
];
const NOT_MODELLED: &[&str] = &[
    "mail that arrives, and releases a person makes, after this measurement and before the re-judge",
    "conversations created or removed while the pages were walked can shift one page boundary",
];

fn add_map(into: &mut Map<String, Value>, from: &Value) {
    for (k, v) in from.as_object().into_iter().flatten() {
        let n = into.get(k).and_then(Value::as_u64).unwrap_or(0) + v.as_u64().unwrap_or(0);
        into.insert(k.clone(), n.into());
    }
}

/// The pages summed, after checking they are one complete, dry walk with
/// one candidate, and that candidate is the report's.
pub fn merge(pages: &str, candidate: &str) -> Result<Value, String> {
    let pages: Vec<Value> = pages
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("live page: {e}"))?;
    let Some(last) = pages.last() else {
        return Err("no live pages".into());
    };
    if last["done"] != true {
        return Err("the live walk did not reach the end".into());
    }
    let mut next = 0u64;
    let mut walk = Map::new();
    let mut counts = Map::new();
    let mut maps: Map<String, Value> = Map::new();
    let mut by_rule: Map<String, Value> = Map::new();
    let mut domains = BTreeSet::new();
    let mut samples = Vec::new();
    let mut shadow = Map::new();
    let mut shadow_samples = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        if page["dry_run"] != true || page["candidate"] != candidate {
            return Err(format!(
                "live page {i}: not a dry run with {candidate}: dry_run={} candidate={}",
                page["dry_run"], page["candidate"]
            ));
        }
        let walked = page["threads_walked"]
            .as_u64()
            .ok_or("missing threads_walked")?;
        let end = page["next_skip"].as_u64().ok_or("missing next_skip")?;
        if end.checked_sub(walked) != Some(next) {
            return Err(format!(
                "live page {i} starts at {}, not {next}",
                end.saturating_sub(walked)
            ));
        }
        next = end;
        for k in WALK {
            let n = page[k]
                .as_u64()
                .ok_or(format!("live page {i}: missing {k}"))?;
            let sum = walk.get(*k).and_then(Value::as_u64).unwrap_or(0) + n;
            walk.insert(k.to_string(), sum.into());
        }
        let f = &page["from_stored"];
        for k in COUNTS {
            let n = f[k].as_u64().ok_or(format!("live page {i}: missing {k}"))?;
            let sum = counts.get(*k).and_then(Value::as_u64).unwrap_or(0) + n;
            counts.insert(k.to_string(), sum.into());
        }
        for k in MAPS {
            let into = maps
                .entry(k.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            add_map(into.as_object_mut().ok_or("map")?, &f[k]);
        }
        for (m, rules) in f["by_rule"].as_object().into_iter().flatten() {
            let into = by_rule
                .entry(m.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            add_map(into.as_object_mut().ok_or("map")?, rules);
        }
        domains.extend(
            f["out_of_inbox_sender_domains"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d.as_str().map(str::to_string)),
        );
        for s in f["samples"].as_array().into_iter().flatten() {
            if samples.len() < 25 {
                samples.push(s.clone());
            }
        }
        add_map(&mut shadow, &page["shadow_by_rule"]);
        for s in page["shadow_samples"].as_array().into_iter().flatten() {
            if shadow_samples.len() < 50 {
                shadow_samples.push(s.clone());
            }
        }
    }
    let mut out = Map::new();
    out.insert("candidate".into(), candidate.into());
    out.insert("pages".into(), pages.len().into());
    out.extend(walk);
    out.extend(counts);
    out.extend(maps);
    out.insert(
        "out_of_inbox_sender_domains".into(),
        serde_json::json!(domains),
    );
    out.insert("by_rule".into(), Value::Object(by_rule));
    out.insert("samples".into(), samples.into());
    // conversations each shadow rule hit; moves nothing, so it is apart
    // from the counts above
    out.insert("shadow_by_rule".into(), Value::Object(shadow));
    out.insert("shadow_samples".into(), shadow_samples.into());
    out.insert("not_modelled".into(), serde_json::json!(NOT_MODELLED));
    Ok(Value::Object(out))
}

/// The gate report: stored against candidate first, pack against pack second.
pub fn gate(report: Value, pages: &str) -> Result<Value, String> {
    let candidate = report["candidate"]
        .as_str()
        .ok_or("the compare report names no candidate")?
        .to_string();
    Ok(serde_json::json!({
        "live_vs_candidate": merge(pages, &candidate)?,
        "pack_vs_pack": report,
    }))
}

/// The summary lines for the stored-against-candidate block.
pub fn summary(live: &Value) -> Vec<String> {
    let n = |k: &str| live[k].to_string();
    let mut lines = vec![
        "== live vs candidate: stored state now against the candidate judged by this binary (what the re-judge will change) ==".to_string(),
        format!(
            "holds: {} new (from {}), {} released (to {})",
            n("would_hold"),
            live["would_hold_from"],
            n("would_release"),
            live["would_release_to"]
        ),
        format!("would move to Junk: {}", n("would_move_to_junk")),
        format!("would move out of Junk: {}", n("would_move_out_of_junk")),
        format!("would move to inbox: {}", n("would_move_to_inbox")),
        format!("would move out of inbox: {}", n("would_move_out_of_inbox")),
    ];
    let domains: Vec<&str> = live["out_of_inbox_sender_domains"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d.as_str())
        .collect();
    if !domains.is_empty() {
        lines.push(format!(
            "out of inbox, sender domains: {}",
            domains.join(", ")
        ));
    }
    lines.push(format!("by rule: {}", live["by_rule"]));
    lines.push(format!(
        "shadow hits by rule (conversations; nothing moved): {}",
        live["shadow_by_rule"]
    ));
    lines.push(format!(
        "conversations: {} walked, {} without a file ({} of them held), {} failed, {} kept released by a person, {} whitelisted from Junk",
        n("threads_walked"),
        n("no_file"),
        n("held_but_unreadable"),
        n("verdict_failed"),
        n("kept_released"),
        n("scored_but_whitelisted")
    ));
    for m in live["not_modelled"].as_array().into_iter().flatten() {
        lines.push(format!("not modelled: {}", m.as_str().unwrap_or("")));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(skip: u64, walked: u64, done: bool, hold: u64, domain: &str) -> String {
        serde_json::json!({
            "dry_run": true, "candidate": "lua:c", "done": done,
            "next_skip": skip + walked, "threads_walked": walked,
            "no_file": 1, "verdict_failed": 0, "kept_released": 0,
            "held_but_unreadable": 0, "scored_but_whitelisted": 0,
            "shadow_by_rule": {"s": hold},
            "shadow_samples": [{"rule": "s"}],
            "from_stored": {
                "would_hold": hold, "would_release": 0, "would_move_to_junk": 0,
                "would_move_out_of_junk": 0, "would_move_to_inbox": 0,
                "would_move_out_of_inbox": hold,
                "would_hold_from": {"inbox": hold}, "would_release_to": {},
                "out_of_inbox_sender_domains": [domain],
                "by_rule": {"hold": {"r": hold}}, "samples": [],
            },
        })
        .to_string()
    }

    #[test]
    fn pages_are_summed() {
        let pages = format!(
            "{}\n{}\n",
            page(0, 2, false, 1, "a.invalid"),
            page(2, 1, true, 2, "b.invalid")
        );
        let m = merge(&pages, "lua:c").expect("merge");
        assert_eq!(m["would_hold"], 3);
        assert_eq!(m["would_hold_from"]["inbox"], 3);
        assert_eq!(m["threads_walked"], 3);
        assert_eq!(m["no_file"], 2);
        assert_eq!(m["by_rule"]["hold"]["r"], 3);
        // shadow hits are summed apart from what moves
        assert_eq!(m["shadow_by_rule"]["s"], 3);
        assert_eq!(m["shadow_samples"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            m["out_of_inbox_sender_domains"],
            serde_json::json!(["a.invalid", "b.invalid"])
        );
    }

    #[test]
    fn a_gap_an_unfinished_walk_or_another_pack_is_refused() {
        let gap = format!(
            "{}\n{}\n",
            page(0, 2, false, 0, "a"),
            page(3, 1, true, 0, "a")
        );
        assert!(merge(&gap, "lua:c").is_err());
        assert!(merge(&page(0, 2, false, 0, "a"), "lua:c").is_err());
        assert!(merge(&page(0, 2, true, 0, "a"), "lua:other").is_err());
        assert!(merge("", "lua:c").is_err());
    }

    /// The r5 shape: the packs agree, the stored state does not.
    #[test]
    fn the_gate_reports_the_stored_comparison_beside_a_zero_pack_diff() {
        let report =
            serde_json::json!({"candidate": "lua:c", "added_holds": 0, "removed_holds": 0});
        let g = gate(report, &page(0, 5, true, 5, "mpengg.com")).expect("gate");
        assert_eq!(g["live_vs_candidate"]["would_hold"], 5);
        assert_eq!(g["pack_vs_pack"]["added_holds"], 0);
    }
}
