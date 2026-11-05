//! The daemon's own SQLite file, on its own volume.
//!
//! Tables are named for what they hold: `_events` and `_log` are facts,
//! appended and never changed; `_computed` is derived from them and may be
//! overwritten. `alert_state_computed` carries the state machine's timers
//! as well as its phase; when a row is missing the phase is rebuilt from
//! the last transition and the timers start again.
//!
//! Retention for phase 1: heartbeat reports 14 days, the cycle log 30 days,
//! the notification log one year; everything else is kept.

mod alerts;
mod control;
mod log;
mod reports;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub use alerts::Transition;
pub use control::{Maintenance, Silence};
pub use log::{CycleRow, NotificationRow};
pub use reports::{StartRow, StoredReport};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS alert_transitions_events (
    id INTEGER PRIMARY KEY,
    rule TEXT NOT NULL,
    object TEXT NOT NULL,
    from_phase TEXT NOT NULL,
    to_phase TEXT NOT NULL,
    at INTEGER NOT NULL,
    value TEXT,
    reason TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS alert_transitions_at ON alert_transitions_events (at);
CREATE INDEX IF NOT EXISTS alert_transitions_rule ON alert_transitions_events (rule, object, at);
CREATE TABLE IF NOT EXISTS alert_state_computed (
    rule TEXT NOT NULL,
    object TEXT NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (rule, object)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS notifications_log (
    id INTEGER PRIMARY KEY,
    group_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    result TEXT NOT NULL,
    at INTEGER NOT NULL,
    summary TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS notifications_at ON notifications_log (at);
CREATE TABLE IF NOT EXISTS silences_events (
    id INTEGER PRIMARY KEY,
    silence_id TEXT NOT NULL,
    action TEXT NOT NULL,
    rule TEXT NOT NULL,
    object TEXT,
    until INTEGER NOT NULL,
    reason TEXT NOT NULL,
    by TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS maintenance_state_events (
    id INTEGER PRIMARY KEY,
    action TEXT NOT NULL,
    until INTEGER NOT NULL,
    reason TEXT NOT NULL,
    by TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS cycle_log (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    sources TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS cycle_log_at ON cycle_log (started_at);
CREATE TABLE IF NOT EXISTS process_reports_events (
    id INTEGER PRIMARY KEY,
    process TEXT NOT NULL,
    instance TEXT NOT NULL,
    start_count INTEGER,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    sent_at INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    kevy_contact_at INTEGER,
    report TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS process_reports_key ON process_reports_events (process, instance, received_at);
CREATE INDEX IF NOT EXISTS process_reports_at ON process_reports_events (received_at);
CREATE TABLE IF NOT EXISTS process_starts_events (
    id INTEGER PRIMARY KEY,
    process TEXT NOT NULL,
    instance TEXT NOT NULL,
    start_count INTEGER,
    how TEXT NOT NULL,
    starts INTEGER NOT NULL,
    between_from INTEGER NOT NULL,
    between_to INTEGER NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS process_starts_key ON process_starts_events (process, instance, at);
CREATE TABLE IF NOT EXISTS report_rejections_log (
    id INTEGER PRIMARY KEY,
    process TEXT NOT NULL,
    reason TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS channel_tests_events (
    id INTEGER PRIMARY KEY,
    config TEXT NOT NULL,
    results TEXT NOT NULL,
    at INTEGER NOT NULL
);
";

pub struct Store(Mutex<rusqlite::Connection>);

#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StoreError(e.to_string()))?;
        }
        let conn = rusqlite::Connection::open(path)?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(rusqlite::Connection::open_in_memory()?)
    }

    fn init(conn: rusqlite::Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store(Mutex::new(conn)))
    }

    fn conn(&self) -> MutexGuard<'_, rusqlite::Connection> {
        // a panic while holding the lock leaves a connection that is still
        // usable: SQLite rolls back the statement it was in
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Delete what is past its retention. Run once a cycle; cheap when
    /// nothing is due because every delete is by an indexed time.
    pub fn prune(&self, now: i64) -> Result<()> {
        let c = self.conn();
        c.execute(
            "DELETE FROM process_reports_events WHERE kind = 'heartbeat' AND received_at < ?1",
            [now - 14 * 86_400],
        )?;
        c.execute(
            "DELETE FROM cycle_log WHERE started_at < ?1",
            [now - 30 * 86_400],
        )?;
        c.execute(
            "DELETE FROM notifications_log WHERE at < ?1",
            [now - 365 * 86_400],
        )?;
        Ok(())
    }
}
