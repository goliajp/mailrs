//! Persistent store trait for TLSRPT event facts.
//!
//! Facts (success / failure observations) are append-only. The
//! report-generation path drains a time window of facts and feeds
//! them into a [`crate::ReportBuilder`].
//!
//! This trait + the [`InMemoryStore`] reference impl mirror the
//! shape of `mailrs_mta_sts::Cache`: a small async trait the
//! caller plugs into, with one in-memory impl for tests and
//! single-process deployments. A Postgres-backed impl lives in
//! the server crate.
//!
//! ## Data-architecture lens
//!
//! Per `common/data-architecture.md`, facts are the source of
//! truth (append-only, immutable). [`crate::Report`] is a
//! derivation — recomputable from the facts at any time. Storing
//! both is fine; the contract is that any disagreement between a
//! cached report and a recomputation from facts is resolved by
//! recomputing.
//!
//! ## Why drain instead of read
//!
//! Reports are generated once per window. After a window's
//! report has been built and submitted, the facts are no longer
//! needed by the reporter (audit / debugging may keep them
//! elsewhere). Drain semantics mean the impl can DELETE the
//! drained rows in the same transaction as returning them — no
//! risk of double-reporting if the server crashes between
//! "drain" and "submit". The submission worker rebuilds the
//! report from the returned facts, so a crash after drain just
//! loses one window of submission (the facts themselves are
//! gone, but every TLSRPT receiver tolerates occasional gaps per
//! the spec).

use std::sync::Mutex;

use async_trait::async_trait;

use crate::report::{FailureEvent, ReportBuilder, SuccessEvent};

/// One persisted event fact — either a successful TLS session
/// observation or a structured failure observation.
///
/// Recorded at `recorded_at_unix_secs` (epoch seconds in UTC).
/// The store uses this timestamp for window queries; the event's
/// own implicit timestamp (the moment the observation happened)
/// is the same as `recorded_at` because the observer records
/// synchronously.
#[derive(Debug, Clone, PartialEq)]
pub enum EventFact {
    /// One successful TLS session.
    Success(SuccessEvent),
    /// One failed TLS attempt with structured context.
    Failure(FailureEvent),
}

impl EventFact {
    /// Apply this fact to a [`ReportBuilder`]. Delegates to
    /// `record_success` / `record_failure`.
    pub fn apply(&self, builder: &mut ReportBuilder) {
        match self {
            Self::Success(e) => builder.record_success(e.clone()),
            Self::Failure(e) => builder.record_failure(e.clone()),
        }
    }
}

/// Errors returned by [`Store`] implementations.
#[derive(Debug)]
pub enum StoreError {
    /// Backend-specific error (PG row insert failed, file write
    /// failed, etc.). Boxed for object-safety; the inner type
    /// carries the diagnostic detail.
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "tls-rpt store backend error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(e) => Some(&**e),
        }
    }
}

/// Persistent store for TLSRPT event facts.
///
/// **Append-only.** New observations always `append`; never
/// update or delete in place. The only deletion is via
/// [`drain_window`](Self::drain_window) which is part of the
/// report-generation flow.
#[async_trait]
pub trait Store: Send + Sync {
    /// Persist one event fact, recorded at `recorded_at_unix_secs`.
    /// Backend impls should make this fast — it's called once per
    /// outbound TLS attempt.
    async fn append(
        &self,
        event: EventFact,
        recorded_at_unix_secs: u64,
    ) -> Result<(), StoreError>;

    /// Atomically return + delete all facts whose
    /// `recorded_at_unix_secs` falls in `[start, end)`. Order is
    /// not specified; callers feed each into a builder in any
    /// order.
    ///
    /// "Atomic" means: either the caller gets every fact in the
    /// window AND they're gone from the store, or it gets none of
    /// them AND nothing was deleted. No middle ground (e.g.
    /// returned 5 of 10 then crashed).
    async fn drain_window(
        &self,
        start_unix_secs: u64,
        end_unix_secs: u64,
    ) -> Result<Vec<EventFact>, StoreError>;
}

/// In-memory reference implementation. `Mutex<Vec<(u64, EventFact)>>`
/// — sufficient for single-process MTAs and tests. Production
/// deployments expecting to survive a restart should plug in a
/// PG-backed impl.
pub struct InMemoryStore {
    rows: Mutex<Vec<(u64, EventFact)>>,
}

impl InMemoryStore {
    /// New empty in-memory store.
    pub fn new() -> Self {
        Self {
            rows: Mutex::new(Vec::new()),
        }
    }

    /// Number of currently-stored events. Test-only.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.rows.lock().unwrap().len()
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn append(
        &self,
        event: EventFact,
        recorded_at_unix_secs: u64,
    ) -> Result<(), StoreError> {
        self.rows
            .lock()
            .unwrap()
            .push((recorded_at_unix_secs, event));
        Ok(())
    }

    async fn drain_window(
        &self,
        start_unix_secs: u64,
        end_unix_secs: u64,
    ) -> Result<Vec<EventFact>, StoreError> {
        let mut rows = self.rows.lock().unwrap();
        let mut keep = Vec::with_capacity(rows.len());
        let mut drained = Vec::new();
        for (ts, ev) in rows.drain(..) {
            if ts >= start_unix_secs && ts < end_unix_secs {
                drained.push(ev);
            } else {
                keep.push((ts, ev));
            }
        }
        *rows = keep;
        Ok(drained)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::PolicyType;

    fn s(domain: &str) -> EventFact {
        EventFact::Success(SuccessEvent {
            policy_domain: domain.into(),
            policy_type: PolicyType::NoPolicyFound,
            mx_host: "mx.example.com".into(),
        })
    }

    #[tokio::test]
    async fn in_memory_append_and_drain_round_trip() {
        let store = InMemoryStore::new();
        store.append(s("a.example"), 100).await.unwrap();
        store.append(s("b.example"), 200).await.unwrap();
        store.append(s("c.example"), 300).await.unwrap();
        let drained = store.drain_window(150, 250).await.unwrap();
        assert_eq!(drained.len(), 1);
        match &drained[0] {
            EventFact::Success(e) => assert_eq!(e.policy_domain, "b.example"),
            EventFact::Failure(_) => panic!("expected Success"),
        }
        // remaining rows: 100 and 300 (outside window)
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn drain_window_is_half_open_excludes_end() {
        let store = InMemoryStore::new();
        store.append(s("x.example"), 100).await.unwrap();
        store.append(s("y.example"), 200).await.unwrap();
        // [100, 200) — should return only the 100 event.
        let drained = store.drain_window(100, 200).await.unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(store.len(), 1); // 200 stays
    }

    #[tokio::test]
    async fn drain_empty_window_returns_empty_and_keeps_rows() {
        let store = InMemoryStore::new();
        store.append(s("x.example"), 100).await.unwrap();
        let drained = store.drain_window(200, 300).await.unwrap();
        assert!(drained.is_empty());
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn drain_includes_start_boundary() {
        let store = InMemoryStore::new();
        store.append(s("x.example"), 100).await.unwrap();
        let drained = store.drain_window(100, 200).await.unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn event_fact_apply_to_builder() {
        let store = InMemoryStore::new();
        store.append(s("a.example"), 100).await.unwrap();
        let drained = store.drain_window(0, 1000).await.unwrap();
        assert_eq!(drained.len(), 1);
        let mut builder = ReportBuilder::new()
            .organization_name("Org")
            .contact_info("mailto:t@e.com")
            .report_id("r")
            .date_range("a", "b");
        for fact in &drained {
            fact.apply(&mut builder);
        }
        let report = builder.build().unwrap();
        assert_eq!(report.policies.len(), 1);
        assert_eq!(report.policies[0].policy.policy_domain, "a.example");
    }
}
