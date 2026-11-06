//! Notices held back while maintenance is on. Each is a fact, written as it
//! is held, so the summary sent when maintenance ends still lists them
//! after the daemon restarted in between.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Result, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldRow {
    pub rule: String,
    pub kind: String,
    pub severity: String,
    pub object: String,
    pub value: String,
    pub threshold: String,
    pub since: i64,
    /// When it was held.
    pub at: i64,
}

impl Store {
    pub fn record_held(&self, h: &HeldRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO maintenance_held_events (rule, kind, severity, object, value, threshold, since, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![h.rule, h.kind, h.severity, h.object, h.value, h.threshold, h.since, h.at],
        )?;
        Ok(())
    }

    /// What was held from `since` on, oldest first: one stretch of
    /// maintenance when `since` is when it started.
    pub fn held_since(&self, since: i64) -> Result<Vec<HeldRow>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT rule, kind, severity, object, value, threshold, since, at
             FROM maintenance_held_events WHERE at >= ?1 ORDER BY id",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(HeldRow {
                rule: r.get(0)?,
                kind: r.get(1)?,
                severity: r.get(2)?,
                object: r.get(3)?,
                value: r.get(4)?,
                threshold: r.get(5)?,
                since: r.get(6)?,
                at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
