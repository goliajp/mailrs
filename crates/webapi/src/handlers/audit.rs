//! Unified audit log (G12) — append-only fact stream in network kevy.
//!
//! Every admin-side mutation records one immutable fact. Per the
//! data-architecture rules audit rows are FACTS: append-only, never
//! updated, carrying both `occurred_at` (when the action happened) and
//! `recorded_at` (when we logged it — identical here since we log
//! synchronously, but kept distinct so the schema doesn't lie).
//!
//! Storage: `admin:audit_log` list (newest first via LPUSH), capped at
//! `AUDIT_CAP` entries so it can't grow unbounded — the oldest fact
//! ages out (retention). A monotonic counter gives each row a stable id.

use serde::Serialize;

use crate::handlers::kevy_util::with_kevy;

const AUDIT_KEY: &[u8] = b"admin:audit_log";
const AUDIT_CTR: &str = "admin:audit_log:counter";
/// Retention cap — ~90 days of typical admin activity.
const AUDIT_CAP: i64 = 50_000;

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

/// Record one audit fact. Best-effort + fire-and-forget: an audit
/// write must never fail the operation it describes. Actor/action are
/// cheap `&str`; the row is LPUSH'd so newest reads first.
pub fn record(actor: &str, action: &str, target: &str, detail: &str) {
    let actor = actor.to_string();
    let action = action.to_string();
    let target = target.to_string();
    let detail = detail.to_string();
    let now = now_secs();
    let _ = with_kevy(move |c| {
        let id = c.incr(AUDIT_CTR.as_bytes())?;
        let fact = AuditFact {
            id,
            occurred_at: now,
            recorded_at: now,
            actor,
            action,
            target,
            detail,
        };
        let payload = serde_json::to_vec(&fact).unwrap_or_default();
        c.lpush(AUDIT_KEY, &[payload.as_slice()])?;
        // retention: kevy-client has no LTRIM, so when the list grows a
        // window past the cap, rewrite it to the newest AUDIT_CAP rows.
        // Amortized cheap — the rewrite fires once per AUDIT_CAP/10 rows.
        let len = c.llen(AUDIT_KEY)? as i64;
        if len > AUDIT_CAP + AUDIT_CAP / 10 {
            let keep = c.lrange(AUDIT_KEY, 0, AUDIT_CAP - 1)?;
            c.del(&[AUDIT_KEY])?;
            // re-push oldest-first so LPUSH restores newest-first order
            for row in keep.iter().rev() {
                c.lpush(AUDIT_KEY, &[row.as_slice()])?;
            }
        }
        Ok(())
    });
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
