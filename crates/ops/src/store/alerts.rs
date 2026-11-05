//! Alert state changes (facts) and the current state (derived).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Result, Store};

/// One recorded change of an alert's phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub rule: String,
    pub object: String,
    pub from: String,
    pub to: String,
    pub at: i64,
    pub value: Option<String>,
    pub reason: String,
}

impl Store {
    pub fn record_transition(&self, t: &Transition) -> Result<()> {
        self.conn().execute(
            "INSERT INTO alert_transitions_events (rule, object, from_phase, to_phase, at, value, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![t.rule, t.object, t.from, t.to, t.at, t.value, t.reason],
        )?;
        Ok(())
    }

    pub fn save_state(&self, rule: &str, object: &str, state: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO alert_state_computed (rule, object, state) VALUES (?1, ?2, ?3)
             ON CONFLICT (rule, object) DO UPDATE SET state = excluded.state",
            params![rule, object, state],
        )?;
        Ok(())
    }

    /// Every saved state: `(rule, object, state json)`.
    pub fn load_states(&self) -> Result<Vec<(String, String, String)>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT rule, object, state FROM alert_state_computed")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The phase each (rule, object) was last moved to, for rebuilding a
    /// state whose derived row is gone.
    pub fn last_phases(&self) -> Result<Vec<(String, String, String, i64)>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT t.rule, t.object, t.to_phase, t.at FROM alert_transitions_events t
             JOIN (SELECT rule, object, MAX(id) AS id FROM alert_transitions_events GROUP BY rule, object) m
             ON t.id = m.id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    #[cfg(test)]
    pub fn clear_states_for_test(&self) {
        self.conn()
            .execute("DELETE FROM alert_state_computed", [])
            .unwrap();
    }

    /// State changes at or after `since`, newest first.
    pub fn transitions_since(&self, since: i64, limit: u32) -> Result<Vec<Transition>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT rule, object, from_phase, to_phase, at, value, reason
             FROM alert_transitions_events WHERE at >= ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![since, limit], |r| {
            Ok(Transition {
                rule: r.get(0)?,
                object: r.get(1)?,
                from: r.get(2)?,
                to: r.get(3)?,
                at: r.get(4)?,
                value: r.get(5)?,
                reason: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
