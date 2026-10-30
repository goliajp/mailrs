//! The audit log: one immutable fact per admin-side action, appended by
//! whichever process took it. Newest first, capped at [`CAP`] rows; a
//! counter gives each row a stable id.

use serde::Serialize;

use crate::keys::admin::{AUDIT_LOG, AUDIT_LOG_COUNTER};

/// Retention cap — ~90 days of typical admin activity.
pub const CAP: i64 = 50_000;

/// One immutable audit fact.
#[derive(Debug, Clone, Serialize)]
pub struct AuditFact {
    /// Monotonic row id.
    pub id: i64,
    /// When the action happened (unix seconds). Fact time.
    pub occurred_at: i64,
    /// When we recorded it. System time.
    pub recorded_at: i64,
    /// Authenticated actor (email) or "system".
    pub actor: String,
    /// Verb: `account.create`, `alias.delete`, `sieve.update`, …
    pub action: String,
    /// What was acted on (address / id / name).
    pub target: String,
    /// Free-text context (old→new, reason, …).
    pub detail: String,
}

/// Append one fact recorded at `now`.
pub fn append(
    c: &mut kevy_client::Connection,
    actor: &str,
    action: &str,
    target: &str,
    detail: &str,
    now: i64,
) -> std::io::Result<()> {
    let fact = AuditFact {
        id: c.incr(AUDIT_LOG_COUNTER)?,
        occurred_at: now,
        recorded_at: now,
        actor: actor.to_string(),
        action: action.to_string(),
        target: target.to_string(),
        detail: detail.to_string(),
    };
    let payload = serde_json::to_vec(&fact).unwrap_or_default();
    c.lpush(AUDIT_LOG, &[payload.as_slice()])?;
    // kevy-client has no LTRIM: when the list grows a window past the cap,
    // rewrite it to the newest CAP rows, once per CAP/10 rows
    let len = c.llen(AUDIT_LOG)? as i64;
    if len > CAP + CAP / 10 {
        let keep = c.lrange(AUDIT_LOG, 0, CAP - 1)?;
        c.del(&[AUDIT_LOG])?;
        for row in keep.iter().rev() {
            c.lpush(AUDIT_LOG, &[row.as_slice()])?;
        }
    }
    Ok(())
}
