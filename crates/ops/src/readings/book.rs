//! Reports received from the processes: checked, recorded, and the starts
//! read off their counters.
//!
//! A report is kept only if it is signed with the key of the process it
//! names, was built within five minutes of the daemon's clock, and carries
//! a sequence number above every one already seen for the same process,
//! instance and start.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mailrs_ops_report::wire::{PROCESSES, Report, ReportKind};

use crate::store::{StartRow, Store, StoredReport};

/// The largest difference between a report's time and ours.
pub const MAX_SKEW_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Unreadable,
    NoMasterKey,
    UnknownProcess,
    BadSignature,
    Clock,
    Replayed,
    Store,
}

impl Rejection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rejection::Unreadable => "unreadable",
            Rejection::NoMasterKey => "no master key configured",
            Rejection::UnknownProcess => "unknown process",
            Rejection::BadSignature => "bad signature",
            Rejection::Clock => "sent time too far from ours",
            Rejection::Replayed => "sequence number already seen",
            Rejection::Store => "could not be recorded",
        }
    }
}

pub struct ReportBook {
    store: Arc<Store>,
    master: Option<Vec<u8>>,
    latest: Mutex<HashMap<(String, String), StoredReport>>,
}

impl ReportBook {
    pub fn load(store: Arc<Store>, master: Option<&str>) -> crate::store::Result<Self> {
        let latest = store
            .latest_reports()?
            .into_iter()
            .map(|r| ((r.report.process.clone(), r.report.instance.clone()), r))
            .collect();
        Ok(ReportBook {
            store,
            master: master.map(|m| m.as_bytes().to_vec()),
            latest: Mutex::new(latest),
        })
    }

    /// Check and record one report body.
    pub fn receive(&self, body: &[u8], signature: &str, now: i64) -> Result<(), Rejection> {
        let report: Report = serde_json::from_slice(body).map_err(|_| Rejection::Unreadable)?;
        let verdict = self.check(&report, body, signature, now);
        if let Err(r) = &verdict {
            let name = match PROCESSES.contains(&report.process.as_str()) {
                true => report.process.as_str(),
                false => "(other)",
            };
            if let Err(e) = self.store.record_rejection(name, r.as_str(), now) {
                tracing::error!(error = %e, "report rejection not recorded");
            }
            tracing::warn!(process = name, reason = r.as_str(), "report rejected");
        }
        verdict
    }

    fn check(
        &self,
        report: &Report,
        body: &[u8],
        signature: &str,
        now: i64,
    ) -> Result<(), Rejection> {
        let master = self.master.as_deref().ok_or(Rejection::NoMasterKey)?;
        if !PROCESSES.contains(&report.process.as_str()) {
            return Err(Rejection::UnknownProcess);
        }
        if !mailrs_ops_report::key::verify(master, &report.process, body, signature) {
            return Err(Rejection::BadSignature);
        }
        if (report.sent_at - now).abs() > MAX_SKEW_SECS {
            return Err(Rejection::Clock);
        }
        let key = (report.process.clone(), report.instance.clone());
        let mut latest = self.latest.lock().unwrap_or_else(|p| p.into_inner());
        let prev = latest.get(&key);
        if let Some(p) = prev
            && p.report.start_count == report.start_count
            && p.report.seq >= report.seq
        {
            return Err(Rejection::Replayed);
        }
        for row in starts(prev, report, now) {
            self.store
                .record_start(&row)
                .map_err(|_| Rejection::Store)?;
        }
        self.store
            .record_report(report, now)
            .map_err(|_| Rejection::Store)?;
        latest.insert(
            key,
            StoredReport {
                report: report.clone(),
                received_at: now,
            },
        );
        Ok(())
    }

    /// The latest report of every (process, instance).
    pub fn latest(&self) -> Vec<StoredReport> {
        let latest = self.latest.lock().unwrap_or_else(|p| p.into_inner());
        latest.values().cloned().collect()
    }
}

/// The starts a report reveals, given the instance's previous report.
fn starts(prev: Option<&StoredReport>, r: &Report, now: i64) -> Vec<StartRow> {
    let row = |how: &str, starts: u64, from: i64| StartRow {
        process: r.process.clone(),
        instance: r.instance.clone(),
        start_count: r.start_count,
        how: how.to_string(),
        starts,
        between_from: from,
        between_to: now,
        at: now,
    };
    let is_start = r.kind == ReportKind::Start;
    let reported = || row("reported", 1, r.build.started_at);
    let prev_count = prev.and_then(|p| p.report.start_count);
    let from = prev.map_or(now, |p| p.received_at);
    match (prev_count, r.start_count) {
        (Some(was), Some(now_count)) if now_count > was => {
            // every start after `was` up to this one, less the one this
            // report announces itself
            let unseen = now_count - was - u64::from(is_start);
            let mut v = Vec::new();
            if unseen > 0 {
                v.push(row("unseen", unseen, from));
            }
            if is_start {
                v.push(reported());
            }
            v
        }
        (Some(was), Some(now_count)) if now_count < was => {
            let mut v = vec![row("reset", 0, from)];
            if is_start {
                v.push(reported());
            }
            v
        }
        _ if is_start => vec![reported()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
