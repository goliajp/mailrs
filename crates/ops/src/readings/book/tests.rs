use std::sync::Arc;

use mailrs_ops_report::wire::{Report, ReportKind};

use super::*;
use crate::readings::fixture;

const NOW: i64 = 1_800_000_000;
const MASTER: &str = "master-key";

fn book() -> (ReportBook, Arc<Store>) {
    let store = Arc::new(Store::in_memory().unwrap());
    (
        ReportBook::load(store.clone(), Some(MASTER)).unwrap(),
        store,
    )
}

fn signed(r: &Report, as_process: &str) -> (Vec<u8>, String) {
    let body = serde_json::to_vec(r).unwrap();
    let key = mailrs_ops_report::key::derive(MASTER.as_bytes(), as_process).unwrap();
    let sig = mailrs_ops_report::key::sign(&key, &body);
    (body, sig)
}

fn report(kind: ReportKind, count: u64, seq: u64, at: i64) -> Report {
    let mut r = fixture::report("receiver", at, None);
    r.kind = kind;
    r.start_count = Some(count);
    r.seq = seq;
    r
}

fn send(b: &ReportBook, r: &Report, at: i64) -> Result<(), Rejection> {
    let (body, sig) = signed(r, &r.process);
    b.receive(&body, &sig, at)
}

#[test]
fn a_report_signed_with_another_process_key_is_dropped_and_counted() {
    let (b, store) = book();
    let mut r = report(ReportKind::Heartbeat, 1, 1, NOW);
    r.process = "fastcore".into();
    let (body, sig) = signed(&r, "receiver");
    assert_eq!(b.receive(&body, &sig, NOW), Err(Rejection::BadSignature));
    assert_eq!(
        store.rejection_counts().unwrap(),
        [("fastcore".to_string(), 1)]
    );
    assert!(b.latest().is_empty());
}

#[test]
fn an_unknown_process_name_is_dropped() {
    let (b, _) = book();
    let mut r = report(ReportKind::Heartbeat, 1, 1, NOW);
    r.process = "intruder".into();
    let body = serde_json::to_vec(&r).unwrap();
    assert_eq!(
        b.receive(&body, "sha256=00", NOW),
        Err(Rejection::UnknownProcess)
    );
}

#[test]
fn an_old_report_and_a_repeated_number_are_dropped() {
    let (b, _) = book();
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 1, NOW - 301), NOW),
        Err(Rejection::Clock)
    );
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 1, NOW + 301), NOW),
        Err(Rejection::Clock)
    );
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 2, NOW), NOW),
        Ok(())
    );
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 2, NOW), NOW + 1),
        Err(Rejection::Replayed)
    );
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 1, NOW), NOW + 1),
        Err(Rejection::Replayed)
    );
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 3, NOW), NOW + 1),
        Ok(())
    );
    // a new start numbers its reports from one again
    assert_eq!(
        send(&b, &report(ReportKind::Start, 2, 1, NOW), NOW + 2),
        Ok(())
    );
}

#[test]
fn starts_the_daemon_did_not_see_are_counted_from_the_counter() {
    let (b, store) = book();
    send(&b, &report(ReportKind::Start, 1, 1, NOW), NOW).unwrap();
    send(&b, &report(ReportKind::Heartbeat, 1, 2, NOW + 30), NOW + 30).unwrap();
    // the daemon is away; the receiver starts twice more and reports again
    send(
        &b,
        &report(ReportKind::Heartbeat, 3, 4, NOW + 600),
        NOW + 600,
    )
    .unwrap();
    let rows = store.all_starts_since(0).unwrap();
    let unseen: Vec<_> = rows.iter().filter(|r| r.how == "unseen").collect();
    assert_eq!(unseen.len(), 1);
    assert_eq!(unseen[0].starts, 2);
    assert_eq!(
        (unseen[0].between_from, unseen[0].between_to),
        (NOW + 30, NOW + 600)
    );
    // the counter file cleared: a reset, not a start
    send(&b, &report(ReportKind::Start, 1, 1, NOW + 700), NOW + 700).unwrap();
    let rows = store.all_starts_since(0).unwrap();
    assert_eq!(rows.iter().filter(|r| r.how == "reset").count(), 1);
    let starts: u64 = rows
        .iter()
        .filter(|r| r.how != "reset")
        .map(|r| r.starts)
        .sum();
    assert_eq!(starts, 1 + 2 + 1);
}

#[test]
fn a_start_report_after_a_missed_one_counts_both() {
    let (b, store) = book();
    send(&b, &report(ReportKind::Start, 1, 1, NOW), NOW).unwrap();
    // start 2 died before its report; start 3 reports
    send(&b, &report(ReportKind::Start, 3, 1, NOW + 60), NOW + 60).unwrap();
    let rows = store.all_starts_since(0).unwrap();
    let total: u64 = rows.iter().map(|r| r.starts).sum();
    assert_eq!(total, 3);
}

#[test]
fn without_a_master_key_nothing_is_taken() {
    let store = Arc::new(Store::in_memory().unwrap());
    let b = ReportBook::load(store, None).unwrap();
    assert_eq!(
        send(&b, &report(ReportKind::Start, 1, 1, NOW), NOW),
        Err(Rejection::NoMasterKey)
    );
}

#[test]
fn the_latest_reports_survive_a_restart() {
    let (b, store) = book();
    send(&b, &report(ReportKind::Heartbeat, 1, 7, NOW), NOW).unwrap();
    drop(b);
    let b = ReportBook::load(store, Some(MASTER)).unwrap();
    assert_eq!(b.latest()[0].report.seq, 7);
    assert_eq!(
        send(&b, &report(ReportKind::Heartbeat, 1, 7, NOW), NOW + 1),
        Err(Rejection::Replayed)
    );
}
