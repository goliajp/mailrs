//! The engine over many cycles, on a fake clock: what is notified, what is
//! filed under a dependency, and what survives a restart.

use std::sync::Arc;

use super::*;
use crate::readings::fixture::healthy;
use crate::readings::{Reading, Snapshot};
use crate::store::Store;

const START: i64 = 1_800_000_000;

fn engine() -> (Engine, Arc<Store>) {
    let store = Arc::new(Store::in_memory().unwrap());
    (Engine::load(store.clone(), 600, START).unwrap(), store)
}

/// Run `cycles` one minute apart, each snapshot made by `f`, collecting
/// the notices that would be sent (not filed under another alert).
fn run(
    e: &mut Engine,
    at: &mut i64,
    cycles: usize,
    f: impl Fn(i64) -> Snapshot,
) -> Vec<(String, String, NoticeKind)> {
    let mut out = Vec::new();
    for _ in 0..cycles {
        for n in e.evaluate(&f(*at)) {
            if n.under.is_none() {
                out.push((n.rule.name.to_string(), n.object.clone(), n.kind));
            }
        }
        *at += 60;
    }
    out
}

fn high_memory(now: i64) -> Snapshot {
    let mut s = healthy(now);
    if let Reading::Got(h) = &mut s.fastcore {
        h.memory.as_mut().unwrap().used_bytes = 4 << 30;
    }
    s
}

#[test]
fn a_healthy_deployment_sends_nothing() {
    let (mut e, _) = engine();
    let mut at = START;
    assert!(run(&mut e, &mut at, 30, healthy).is_empty());
}

#[test]
fn an_unreadable_core_is_one_notice_for_the_source_and_nothing_for_its_rules() {
    let (mut e, _) = engine();
    let mut at = START;
    let n = run(&mut e, &mut at, 11, high_memory);
    assert_eq!(
        n,
        [(
            "core_memory_high".into(),
            "fastcore".into(),
            NoticeKind::Firing
        )]
    );
    // fastcore stops answering within five seconds for twenty minutes
    let slow = |now| {
        let mut s = high_memory(now);
        s.fastcore = Reading::Unreadable("no answer within 5s".into());
        s.fastcore_ready = Reading::Unreadable("no answer within 5s".into());
        s.fastcore_ops = Reading::Unreadable("no answer within 5s".into());
        s
    };
    let n = run(&mut e, &mut at, 20, slow);
    assert_eq!(
        n,
        [(
            "source_unreadable.fastcore".into(),
            "fastcore".into(),
            NoticeKind::Firing
        )],
        "no 'resolved' and no repeat for the core's rules"
    );
    assert_eq!(
        e.state("core_memory_high", "fastcore").unwrap().phase,
        Phase::Unknown
    );
    // back, still high: only the source's recovery
    let n = run(&mut e, &mut at, 1, high_memory);
    assert_eq!(
        n,
        [(
            "source_unreadable.fastcore".into(),
            "fastcore".into(),
            NoticeKind::Resolved
        )]
    );
    assert_eq!(
        e.state("core_memory_high", "fastcore").unwrap().phase,
        Phase::Firing
    );
}

fn kevy_down(now: i64) -> Snapshot {
    let mut s = healthy(now);
    s.kevy = Reading::Got(false);
    if let Reading::Got(h) = &mut s.webapi {
        h["kevy"] = false.into();
        for f in ["kevy_server_memory", "quota", "new_domains", "fraud_hits"] {
            h[f] = serde_json::Value::Null;
        }
        h["set_aside"]["sender"] = serde_json::Value::Null;
    }
    for p in s.processes.values_mut() {
        p.latest.kevy_contact_at = Some(now - 3600);
    }
    s
}

#[test]
fn kevy_server_down_is_one_alert_and_what_reads_through_it_is_filed_under_it() {
    let (mut e, _) = engine();
    let mut at = START;
    let n = run(&mut e, &mut at, 30, kevy_down);
    assert_eq!(
        n,
        [(KEVY_ALERT.into(), "kevy-server".into(), NoticeKind::Firing)],
        "the quota, domain and memory rules are unknown, not separate notices"
    );
    assert_eq!(
        e.state("accounts_without_quota", "accounts").unwrap().phase,
        Phase::Unknown
    );
    // the pushed reports still arrive, so their rules stay readable
    assert_eq!(
        e.state("outbound_stuck", "sender").unwrap().phase,
        Phase::Ok
    );
}

const KEVY_ALERT: &str = rules::KEVY_SERVER_UNAVAILABLE;

#[test]
fn no_report_from_anyone_is_one_notice_not_one_per_process() {
    let (mut e, _) = engine();
    let mut at = START;
    let silent = |now| {
        let mut s = healthy(now);
        for p in s.processes.values_mut() {
            p.fresh = false;
        }
        s
    };
    let n = run(&mut e, &mut at, 15, silent);
    assert_eq!(
        n,
        [(
            rules::NO_REPORTS.into(),
            "reports".into(),
            NoticeKind::Firing
        )]
    );
}

#[test]
fn one_process_gone_quiet_is_named() {
    let (mut e, _) = engine();
    let mut at = START;
    let quiet = |now| {
        let mut s = healthy(now);
        s.processes.get_mut("receiver").unwrap().fresh = false;
        s
    };
    let n = run(&mut e, &mut at, 15, quiet);
    assert_eq!(
        n,
        [(
            "source_unreadable.receiver".into(),
            "receiver".into(),
            NoticeKind::Firing
        )]
    );
}

#[test]
fn the_state_survives_a_restart() {
    let (mut e, store) = engine();
    let mut at = START;
    run(&mut e, &mut at, 11, high_memory);
    drop(e);
    let mut e = Engine::load(store.clone(), 600, at).unwrap();
    assert_eq!(
        e.state("core_memory_high", "fastcore").unwrap().phase,
        Phase::Firing
    );
    // still high after the restart: no second firing
    assert!(run(&mut e, &mut at, 5, high_memory).is_empty());
    // the derived rows gone: the phase comes back from the transitions
    store.clear_states_for_test();
    let e = Engine::load(store, 600, at).unwrap();
    assert_eq!(
        e.state("core_memory_high", "fastcore").unwrap().phase,
        Phase::Firing
    );
}

#[test]
fn every_transition_is_recorded() {
    let (mut e, store) = engine();
    let mut at = START;
    run(&mut e, &mut at, 11, high_memory);
    let t = store.transitions_since(0, 100).unwrap();
    let path: Vec<_> = t
        .iter()
        .rev()
        .filter(|t| t.rule == "core_memory_high")
        .map(|t| format!("{}>{}", t.from, t.to))
        .collect();
    assert_eq!(path, ["ok>pending", "pending>firing"]);
}

#[test]
fn stale_fraud_counts_are_unknown_then_one_unreadable_notice_after_the_wait() {
    let (mut e, _) = engine();
    let mut at = START;
    let stale = |now| {
        let mut s = healthy(now);
        if let Reading::Got(h) = &mut s.webapi {
            h["fraud_hits"]["stale"] = true.into();
        }
        s
    };
    // webapi answers throughout, so nothing speaks for the source
    assert!(run(&mut e, &mut at, 10, stale).is_empty());
    assert_eq!(
        e.state("fraud_hits_unreadable", "fraud_hits")
            .unwrap()
            .phase,
        Phase::Unknown
    );
    let n = run(&mut e, &mut at, 5, stale);
    assert_eq!(
        n,
        [(
            "fraud_hits_unreadable".into(),
            "fraud_hits".into(),
            NoticeKind::Unreadable
        )]
    );
    let n = run(&mut e, &mut at, 1, healthy);
    assert_eq!(
        n,
        [(
            "fraud_hits_unreadable".into(),
            "fraud_hits".into(),
            NoticeKind::Readable
        )]
    );
    assert_eq!(
        e.state("fraud_hits_unreadable", "fraud_hits")
            .unwrap()
            .phase,
        Phase::Ok
    );
}
