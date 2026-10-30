//! Unified audit log (G12) — append-only fact stream in network kevy.
//!
//! Every admin-side mutation records one immutable fact. Audit rows
//! are FACTS: append-only, never
//! updated, carrying both `occurred_at` (when the action happened) and
//! `recorded_at` (when we logged it — identical here since we log
//! synchronously, but kept distinct so the schema doesn't lie).
//!
//! Storage: `admin:audit_log` list (newest first via LPUSH), capped at
//! `AUDIT_CAP` entries so it can't grow unbounded — the oldest fact
//! ages out (retention). A monotonic counter gives each row a stable id.

use crate::handlers::kevy_util::with_kevy;

pub use mailrs_core_sidestate::families::audit::AuditFact;

/// Record one audit fact. Best-effort: an audit write must never fail
/// the operation it describes. The row is LPUSH'd so newest reads first.
pub fn record(actor: &str, action: &str, target: &str, detail: &str) {
    let _ = write(actor, action, target, detail);
}

/// Record one audit fact for an operation that must not happen
/// unrecorded: the row is written before the caller goes on, and a write
/// that fails answers 503 so the caller returns nothing.
pub fn record_required(
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
) -> Result<(), axum::http::StatusCode> {
    write(actor, action, target, detail).map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)
}

fn write(
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
) -> Result<(), axum::http::StatusCode> {
    let actor = actor.to_string();
    let action = action.to_string();
    let target = target.to_string();
    let detail = detail.to_string();
    let now = now_secs();
    with_kevy(move |c| {
        mailrs_core_sidestate::families::audit::append(c, &actor, &action, &target, &detail, now)
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
