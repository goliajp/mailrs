//! Silences and the maintenance state: every set, extension, removal and
//! expiry is an event; what is in force now is read from the events.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Result, Store};

/// Who closes what expired on its own.
pub const DAEMON: &str = "ops-daemon";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Silence {
    pub id: String,
    pub rule: String,
    /// `None` silences every object of the rule.
    pub object: Option<String>,
    pub until: i64,
    pub reason: String,
    pub by: String,
    pub set_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maintenance {
    /// When this stretch of maintenance began (the last `start`).
    pub started_at: i64,
    pub until: i64,
    pub reason: String,
    pub by: String,
}

impl Store {
    pub fn set_silence(&self, s: &Silence) -> Result<()> {
        self.conn().execute(
            "INSERT INTO silences_events (silence_id, action, rule, object, until, reason, by, at)
             VALUES (?1, 'set', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![s.id, s.rule, s.object, s.until, s.reason, s.by, s.set_at],
        )?;
        Ok(())
    }

    /// Record the end of silence `id`; false when no such silence is in force.
    pub fn end_silence(
        &self,
        id: &str,
        action: &str,
        by: &str,
        reason: &str,
        now: i64,
    ) -> Result<bool> {
        let Some(s) = self.silences(now)?.into_iter().find(|s| s.id == id) else {
            return Ok(false);
        };
        self.conn().execute(
            "INSERT INTO silences_events (silence_id, action, rule, object, until, reason, by, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![s.id, action, s.rule, s.object, s.until, reason, by, now],
        )?;
        Ok(true)
    }

    /// Silences set and not ended, including any past their expiry that
    /// [`Self::expire_controls`] has not closed yet.
    fn open_silences(&self) -> Result<Vec<Silence>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT silence_id, rule, object, until, reason, by, at FROM silences_events s
             WHERE action = 'set' AND NOT EXISTS (
                SELECT 1 FROM silences_events e WHERE e.silence_id = s.silence_id AND e.action <> 'set')
             ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Silence {
                id: r.get(0)?,
                rule: r.get(1)?,
                object: r.get(2)?,
                until: r.get(3)?,
                reason: r.get(4)?,
                by: r.get(5)?,
                set_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Silences in force at `now`.
    pub fn silences(&self, now: i64) -> Result<Vec<Silence>> {
        Ok(self
            .open_silences()?
            .into_iter()
            .filter(|s| s.until > now)
            .collect())
    }

    pub fn maintenance_event(
        &self,
        action: &str,
        until: i64,
        reason: &str,
        by: &str,
        now: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO maintenance_state_events (action, until, reason, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![action, until, reason, by, now],
        )?;
        Ok(())
    }

    /// The maintenance stretch the events leave open, expired or not.
    fn open_maintenance(&self) -> Result<Option<Maintenance>> {
        let c = self.conn();
        let last: Option<(String, i64, String, String)> = c
            .query_row(
                "SELECT action, until, reason, by FROM maintenance_state_events ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((action, until, reason, by)) = last else {
            return Ok(None);
        };
        if action != "start" && action != "extend" {
            return Ok(None);
        }
        let started_at: i64 = c.query_row(
            "SELECT at FROM maintenance_state_events WHERE action = 'start' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )?;
        Ok(Some(Maintenance {
            started_at,
            until,
            reason,
            by,
        }))
    }

    /// Maintenance in force at `now`.
    pub fn maintenance(&self, now: i64) -> Result<Option<Maintenance>> {
        Ok(self.open_maintenance()?.filter(|m| m.until > now))
    }

    /// Close silences and maintenance whose time has run out, as events
    /// by the daemon. Returns the maintenance stretch that ended, if any.
    pub fn expire_controls(&self, now: i64) -> Result<Option<Maintenance>> {
        for s in self.open_silences()?.into_iter().filter(|s| s.until <= now) {
            self.conn().execute(
                "INSERT INTO silences_events (silence_id, action, rule, object, until, reason, by, at)
                 VALUES (?1, 'expired', ?2, ?3, ?4, 'reached its end', ?5, ?6)",
                params![s.id, s.rule, s.object, s.until, DAEMON, now],
            )?;
        }
        match self.open_maintenance()? {
            Some(m) if m.until <= now => {
                self.maintenance_event("expired", m.until, "reached its end", DAEMON, now)?;
                Ok(Some(m))
            }
            _ => Ok(None),
        }
    }

    /// Every silence and maintenance event since `since`, newest first, as
    /// JSON rows for the history view.
    pub fn control_events_since(&self, since: i64) -> Result<Vec<serde_json::Value>> {
        let c = self.conn();
        let mut out = Vec::new();
        let mut st = c.prepare(
            "SELECT silence_id, action, rule, object, until, reason, by, at FROM silences_events
             WHERE at >= ?1 ORDER BY id DESC LIMIT 200",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(serde_json::json!({
                "kind": "silence", "id": r.get::<_, String>(0)?, "action": r.get::<_, String>(1)?,
                "rule": r.get::<_, String>(2)?, "object": r.get::<_, Option<String>>(3)?,
                "until": r.get::<_, i64>(4)?, "reason": r.get::<_, String>(5)?,
                "by": r.get::<_, String>(6)?, "at": r.get::<_, i64>(7)?,
            }))
        })?;
        out.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        let mut st = c.prepare(
            "SELECT action, until, reason, by, at FROM maintenance_state_events
             WHERE at >= ?1 ORDER BY id DESC LIMIT 200",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(serde_json::json!({
                "kind": "maintenance", "action": r.get::<_, String>(0)?,
                "until": r.get::<_, i64>(1)?, "reason": r.get::<_, String>(2)?,
                "by": r.get::<_, String>(3)?, "at": r.get::<_, i64>(4)?,
            }))
        })?;
        out.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        Ok(out)
    }
}
