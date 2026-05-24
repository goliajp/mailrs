//! Audit log handlers — append-only security record
//! of who did what to whom.

use super::{AuditEntry, DomainStore, Result};

impl DomainStore {
    /// log a security-sensitive action to the audit_log table (fire-and-forget)
    pub async fn log_audit(&self, actor: &str, action: &str, target: &str, detail: &str) {
        if let Ok(pool) = self.pg() {
            let _ = sqlx::query(
                "INSERT INTO audit_log (actor, action, target, detail) VALUES ($1, $2, $3, $4)",
            )
            .bind(actor)
            .bind(action)
            .bind(target)
            .bind(detail)
            .execute(pool)
            .await;
        }
    }

    /// query recent audit log entries
    pub async fn list_audit_log(&self, limit: i64) -> Result<Vec<AuditEntry>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, i64, String, String, String, String)>(
            "SELECT id, EXTRACT(EPOCH FROM timestamp)::bigint, actor, action, target, detail \
             FROM audit_log ORDER BY timestamp DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, timestamp, actor, action, target, detail)| AuditEntry {
                id,
                timestamp,
                actor,
                action,
                target,
                detail,
            })
            .collect())
    }

    /// delete audit log entries older than the given number of days
    pub async fn cleanup_audit_log(&self, retention_days: i64) {
        if let Ok(pool) = self.pg() {
            let _ = sqlx::query("DELETE FROM audit_log WHERE timestamp < now() - make_interval(days => $1)")
                .bind(retention_days)
                .execute(pool)
                .await;
        }
    }
}
