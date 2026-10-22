//! Pluggable storage for DMARC verification results.
//!
//! [`DmarcStore`] is the interface; [`InMemoryDmarcStore`] is a complete
//! reference implementation; [`check_dmarc_store`] checks any
//! implementation against the trait's rules.

use std::sync::Mutex;

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::DmarcResultRecord;

/// Storage for DMARC verification results, bucketed by report day.
///
/// The caller supplies every date, so an implementation never reads the
/// clock: the inbound path records under the day the message arrived (in
/// whatever time zone the reports use), the report task asks for one day,
/// and retention deletes everything before a cutoff day.
///
/// Implementations should be cheap to share (`Arc<Self>`), since the
/// inbound pipeline and the daily report task use the same instance.
#[async_trait]
pub trait DmarcStore: Send + Sync {
    /// Backend-specific error type returned by every trait method.
    type Error: std::fmt::Debug + Send;

    /// Append a verified result under `report_date`. Recording the same
    /// result twice stores it twice: each call is one message.
    async fn record_result(
        &self,
        report_date: NaiveDate,
        record: &DmarcResultRecord,
    ) -> Result<(), Self::Error>;

    /// Every result recorded under `date`, in any order.
    async fn get_results_for_date(
        &self,
        date: NaiveDate,
    ) -> Result<Vec<DmarcResultRecord>, Self::Error>;

    /// Delete every result recorded under a day before `cutoff`; results
    /// recorded under `cutoff` itself stay. Returns how many were deleted.
    async fn cleanup_before(&self, cutoff: NaiveDate) -> Result<u64, Self::Error>;
}

/// In-memory [`DmarcStore`] for tests and single-process deployments. Not
/// durable across restarts.
#[derive(Default)]
pub struct InMemoryDmarcStore {
    rows: Mutex<Vec<(NaiveDate, DmarcResultRecord)>>,
}

impl InMemoryDmarcStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DmarcStore for InMemoryDmarcStore {
    type Error = std::convert::Infallible;

    async fn record_result(
        &self,
        report_date: NaiveDate,
        record: &DmarcResultRecord,
    ) -> Result<(), Self::Error> {
        self.rows
            .lock()
            .unwrap()
            .push((report_date, record.clone()));
        Ok(())
    }

    async fn get_results_for_date(
        &self,
        date: NaiveDate,
    ) -> Result<Vec<DmarcResultRecord>, Self::Error> {
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .filter(|(d, _)| *d == date)
            .map(|(_, r)| r.clone())
            .collect())
    }

    async fn cleanup_before(&self, cutoff: NaiveDate) -> Result<u64, Self::Error> {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|(d, _)| *d >= cutoff);
        Ok((before - rows.len()) as u64)
    }
}

/// Check a [`DmarcStore`] against the trait's rules; panics naming the
/// first one broken. `store` must start empty.
///
/// ```no_run
/// # async fn run() {
/// mailrs_dmarc::store::check_dmarc_store(&mailrs_dmarc::InMemoryDmarcStore::new()).await;
/// # }
/// ```
pub async fn check_dmarc_store<S: DmarcStore>(store: &S) {
    let day = |d| NaiveDate::from_ymd_opt(2026, 5, d).expect("valid date");
    let rec = |ip: &str| DmarcResultRecord::new(ip, "example.com", "pass", "pass", "pass", "none");

    for (d, ip) in [
        (19, "192.0.2.1"),
        (20, "192.0.2.2"),
        (20, "192.0.2.2"),
        (21, "192.0.2.3"),
    ] {
        store
            .record_result(day(d), &rec(ip))
            .await
            .expect("record_result");
    }
    let on_20 = store
        .get_results_for_date(day(20))
        .await
        .expect("get_results_for_date");
    assert_eq!(
        on_20.len(),
        2,
        "record_result: each call is stored, duplicates included"
    );
    assert!(
        on_20
            .iter()
            .all(|r| r.source_ip == "192.0.2.2" && r.from_domain == "example.com"),
        "get_results_for_date: only that day's results, fields intact"
    );
    let empty = store
        .get_results_for_date(day(22))
        .await
        .expect("get_results_for_date");
    assert!(
        empty.is_empty(),
        "get_results_for_date: a day with nothing is empty"
    );

    let deleted = store.cleanup_before(day(20)).await.expect("cleanup_before");
    assert_eq!(
        deleted, 1,
        "cleanup_before: deletes only days before the cutoff"
    );
    let on_19 = store
        .get_results_for_date(day(19))
        .await
        .expect("get_results_for_date");
    assert!(on_19.is_empty(), "cleanup_before: the earlier day is gone");
    let on_20 = store
        .get_results_for_date(day(20))
        .await
        .expect("get_results_for_date");
    assert_eq!(
        on_20.len(),
        2,
        "cleanup_before: the cutoff day itself stays"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_store_meets_the_contract() {
        check_dmarc_store(&InMemoryDmarcStore::new()).await;
    }
}
