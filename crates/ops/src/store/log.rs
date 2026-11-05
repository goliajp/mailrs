//! The cycle log and the notification log.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Result, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleRow {
    pub started_at: i64,
    pub duration_ms: i64,
    /// Each source and whether it was read: `{"fastcore": true, ...}`.
    pub sources: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationRow {
    pub group_id: String,
    pub channel: String,
    pub attempt: i64,
    /// `sent`, `failed: <why>`, `over budget`, `suppressed: <why>`.
    pub result: String,
    pub at: i64,
    pub summary: String,
}

impl Store {
    pub fn record_cycle(&self, c: &CycleRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO cycle_log (started_at, duration_ms, sources) VALUES (?1, ?2, ?3)",
            params![c.started_at, c.duration_ms, c.sources.to_string()],
        )?;
        Ok(())
    }

    pub fn last_cycle(&self) -> Result<Option<CycleRow>> {
        let c = self.conn();
        let row = c
            .query_row(
                "SELECT started_at, duration_ms, sources FROM cycle_log ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(started_at, duration_ms, sources)| CycleRow {
            started_at,
            duration_ms,
            sources: serde_json::from_str(&sources).unwrap_or(serde_json::Value::Null),
        }))
    }

    pub fn record_notification(&self, n: &NotificationRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO notifications_log (group_id, channel, attempt, result, at, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![n.group_id, n.channel, n.attempt, n.result, n.at, n.summary],
        )?;
        Ok(())
    }

    pub fn notifications_since(&self, since: i64, limit: u32) -> Result<Vec<NotificationRow>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT group_id, channel, attempt, result, at, summary FROM notifications_log
             WHERE at >= ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![since, limit], |r| {
            Ok(NotificationRow {
                group_id: r.get(0)?,
                channel: r.get(1)?,
                attempt: r.get(2)?,
                result: r.get(3)?,
                at: r.get(4)?,
                summary: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// A test message went to every channel of configuration `config`.
    pub fn record_channel_test(
        &self,
        config: &str,
        results: &serde_json::Value,
        now: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO channel_tests_events (config, results, at) VALUES (?1, ?2, ?3)",
            params![config, results.to_string(), now],
        )?;
        Ok(())
    }

    /// The configuration and time of the last channel test.
    pub fn last_channel_test(&self) -> Result<Option<(String, i64)>> {
        let c = self.conn();
        Ok(c.query_row(
            "SELECT config, at FROM channel_tests_events ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
    }

    pub fn record_rejection(&self, process: &str, reason: &str, now: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO report_rejections_log (process, reason, at) VALUES (?1, ?2, ?3)",
            params![process, reason, now],
        )?;
        Ok(())
    }

    /// Rejected reports by the process name they carried.
    pub fn rejection_counts(&self) -> Result<Vec<(String, i64)>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT process, COUNT(*) FROM report_rejections_log GROUP BY process ORDER BY process",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
