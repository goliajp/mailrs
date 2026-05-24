//! `PgTlsRptStore` — `mailrs_tls_rpt::Store` impl backed by PG
//! `tls_rpt_events` (append-only).

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_tls_rpt::{
    EventFact, FailureEvent, Store, StoreError, SuccessEvent,
};
use sqlx::PgPool;


use super::convert::{
    canonical_policy_str_to_type, failure_type_str, policy_type_str,
    str_to_failure_type,
};

pub struct PgTlsRptStore {
    pool: PgPool,
}

impl PgTlsRptStore {
    /// New store wrapping the supplied PG pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convenience: wrap into an `Arc<dyn Store>` ready to hand to
    /// [`TlsRptObserver::new`].
    pub fn into_arc(self) -> Arc<dyn Store> {
        Arc::new(self)
    }
}

#[async_trait]
impl Store for PgTlsRptStore {
    async fn append(
        &self,
        event: EventFact,
        recorded_at_unix_secs: u64,
    ) -> Result<(), StoreError> {
        let ts = recorded_at_unix_secs as i64;
        let r = match event {
            EventFact::Success(e) => sqlx::query(
                "INSERT INTO tls_rpt_events
                    (recorded_at_unix, kind, policy_domain, policy_type, mx_host)
                 VALUES ($1, 'success', $2, $3, $4)",
            )
            .bind(ts)
            .bind(e.policy_domain)
            .bind(policy_type_str(e.policy_type))
            .bind(e.mx_host)
            .execute(&self.pool)
            .await,
            EventFact::Failure(e) => sqlx::query(
                "INSERT INTO tls_rpt_events
                    (recorded_at_unix, kind, policy_domain, policy_type, mx_host,
                     result_type, sending_mta_ip, receiving_ip, receiving_mx_helo,
                     additional_information, failure_reason_code)
                 VALUES ($1, 'failure', $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(ts)
            .bind(e.policy_domain)
            .bind(policy_type_str(e.policy_type))
            .bind(e.mx_host)
            .bind(failure_type_str(e.result_type))
            .bind(e.sending_mta_ip.map(|ip| ip.to_string()))
            .bind(e.receiving_ip.map(|ip| ip.to_string()))
            .bind(e.receiving_mx_helo)
            .bind(e.additional_information)
            .bind(e.failure_reason_code)
            .execute(&self.pool)
            .await,
        };
        r.map(|_| ()).map_err(|e| StoreError::Backend(Box::new(e)))
    }

    async fn drain_window(
        &self,
        start_unix_secs: u64,
        end_unix_secs: u64,
    ) -> Result<Vec<EventFact>, StoreError> {
        let start = start_unix_secs as i64;
        let end = end_unix_secs as i64;
        // DELETE ... RETURNING is the atomic drain — selecting +
        // deleting in two separate statements would leave a window
        // for a concurrent reader to see stale rows. PG's
        // RETURNING clause guarantees the deleted rows are
        // captured before they disappear from any other view.
        let rows: Vec<TlsRptRow> = sqlx::query_as::<_, TlsRptRow>(
            "DELETE FROM tls_rpt_events
             WHERE recorded_at_unix >= $1 AND recorded_at_unix < $2
             RETURNING kind, policy_domain, policy_type, mx_host,
                       result_type, sending_mta_ip, receiving_ip,
                       receiving_mx_helo, additional_information,
                       failure_reason_code",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Backend(Box::new(e)))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let policy_type = canonical_policy_str_to_type(&row.policy_type);
            if row.kind == "success" {
                let mx = row.mx_host.unwrap_or_default();
                out.push(EventFact::Success(SuccessEvent {
                    policy_domain: row.policy_domain,
                    policy_type,
                    mx_host: mx,
                }));
            } else {
                let result_type = str_to_failure_type(row.result_type.as_deref().unwrap_or(""));
                out.push(EventFact::Failure(FailureEvent {
                    policy_domain: row.policy_domain,
                    policy_type,
                    mx_host: row.mx_host,
                    result_type,
                    sending_mta_ip: row.sending_mta_ip.and_then(|s| s.parse().ok()),
                    receiving_ip: row.receiving_ip.and_then(|s| s.parse().ok()),
                    receiving_mx_helo: row.receiving_mx_helo,
                    additional_information: row.additional_information,
                    failure_reason_code: row.failure_reason_code,
                }));
            }
        }
        Ok(out)
    }
}

#[derive(sqlx::FromRow)]
struct TlsRptRow {
    kind: String,
    policy_domain: String,
    policy_type: String,
    mx_host: Option<String>,
    result_type: Option<String>,
    sending_mta_ip: Option<String>,
    receiving_ip: Option<String>,
    receiving_mx_helo: Option<String>,
    additional_information: Option<String>,
    failure_reason_code: Option<String>,
}
