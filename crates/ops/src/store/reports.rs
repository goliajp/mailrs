//! Reports as received, and the starts read from them.

use mailrs_ops_report::wire::Report;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Result, Store};

#[derive(Debug, Clone, PartialEq)]
pub struct StoredReport {
    pub report: Report,
    pub received_at: i64,
}

/// Starts of one instance: one the daemon was told of (`reported`), starts
/// it can only infer from the counter jumping (`unseen`, `starts` of them,
/// some time between `between_from` and `between_to`), or the counter going
/// down (`reset`, which is not a start).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRow {
    pub process: String,
    pub instance: String,
    pub start_count: Option<u64>,
    pub how: String,
    pub starts: u64,
    pub between_from: i64,
    pub between_to: i64,
    pub at: i64,
}

impl Store {
    pub fn record_report(&self, r: &Report, received_at: i64) -> Result<()> {
        let json = serde_json::to_string(r).map_err(|e| super::StoreError(e.to_string()))?;
        self.conn().execute(
            "INSERT INTO process_reports_events
             (process, instance, start_count, seq, kind, sent_at, received_at, kevy_contact_at, report)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                r.process,
                r.instance,
                r.start_count.map(|n| n as i64),
                r.seq as i64,
                r.kind.as_str(),
                r.sent_at,
                received_at,
                r.kevy_contact_at,
                json
            ],
        )?;
        Ok(())
    }

    /// The latest report of every (process, instance).
    pub fn latest_reports(&self) -> Result<Vec<StoredReport>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT p.report, p.received_at FROM process_reports_events p
             JOIN (SELECT process, instance, MAX(id) AS id FROM process_reports_events
                   GROUP BY process, instance) m ON p.id = m.id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (json, received_at) = row?;
            if let Ok(report) = serde_json::from_str(&json) {
                out.push(StoredReport {
                    report,
                    received_at,
                });
            }
        }
        Ok(out)
    }

    /// Whether `instance` of `process` said it was ready at `start_count`.
    pub fn ready_seen(
        &self,
        process: &str,
        instance: &str,
        start_count: Option<u64>,
    ) -> Result<bool> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM process_reports_events
             WHERE process = ?1 AND instance = ?2 AND kind = 'ready' AND start_count IS ?3",
            params![process, instance, start_count.map(|n| n as i64)],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn record_start(&self, s: &StartRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO process_starts_events
             (process, instance, start_count, how, starts, between_from, between_to, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                s.process,
                s.instance,
                s.start_count.map(|n| n as i64),
                s.how,
                s.starts as i64,
                s.between_from,
                s.between_to,
                s.at
            ],
        )?;
        Ok(())
    }

    /// Start rows of one instance recorded at or after `since`.
    pub fn starts_since(&self, process: &str, instance: &str, since: i64) -> Result<Vec<StartRow>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT process, instance, start_count, how, starts, between_from, between_to, at
             FROM process_starts_events WHERE process = ?1 AND instance = ?2 AND at >= ?3 ORDER BY id",
        )?;
        let rows = st.query_map(params![process, instance, since], |r| {
            Ok(StartRow {
                process: r.get(0)?,
                instance: r.get(1)?,
                start_count: r.get::<_, Option<i64>>(2)?.map(|n| n as u64),
                how: r.get(3)?,
                starts: r.get::<_, i64>(4)? as u64,
                between_from: r.get(5)?,
                between_to: r.get(6)?,
                at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// All start rows since `since`, newest first, for the history view.
    pub fn all_starts_since(&self, since: i64) -> Result<Vec<StartRow>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT process, instance, start_count, how, starts, between_from, between_to, at
             FROM process_starts_events WHERE at >= ?1 ORDER BY id DESC LIMIT 500",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(StartRow {
                process: r.get(0)?,
                instance: r.get(1)?,
                start_count: r.get::<_, Option<i64>>(2)?.map(|n| n as u64),
                how: r.get(3)?,
                starts: r.get::<_, i64>(4)? as u64,
                between_from: r.get(5)?,
                between_to: r.get(6)?,
                at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
