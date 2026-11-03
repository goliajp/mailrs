//! How far each account's usage counter (`mailrs:quota:<user>:used_bytes`
//! in the network kevy, what the quota stage reads) has drifted from what
//! its maildir takes on disk. Read-only: account names and numbers, no
//! file names, no subjects. `mailrs-fastcore-backfill-usage` resets the
//! counters to the disk figures.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::FastcoreState;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Row {
    account: String,
    /// The counter; `None` when the account has none.
    counter: Option<i64>,
    /// Bytes its maildir takes.
    actual: u64,
    /// `counter - actual`; `None` without a counter.
    diff: Option<i64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Drift {
    accounts: usize,
    /// Accounts whose counter is not the disk figure, missing ones included.
    differing: usize,
    /// Accounts the backfill would raise: counter below the disk figure, or
    /// missing while the maildir holds mail.
    counter_low: Vec<String>,
    rows: Vec<Row>,
}

/// The drift of each of `usage` against the counters `counter` reads.
pub(crate) fn drift(
    usage: Vec<(String, u64)>,
    mut counter: impl FnMut(&str) -> std::io::Result<Option<i64>>,
) -> std::io::Result<Drift> {
    let mut out = Drift {
        accounts: usage.len(),
        differing: 0,
        counter_low: Vec::new(),
        rows: Vec::with_capacity(usage.len()),
    };
    for (account, actual) in usage {
        let have = counter(&account)?;
        let actual_i = i64::try_from(actual).unwrap_or(i64::MAX);
        let diff = have.map(|c| c - actual_i);
        if diff != Some(0) {
            out.differing += 1;
        }
        let low = match have {
            Some(c) => c < actual_i,
            None => actual > 0,
        };
        if low {
            out.counter_low.push(account.clone());
        }
        out.rows.push(Row {
            account,
            counter: have,
            actual,
            diff,
        });
    }
    Ok(out)
}

/// `GET /v1/admin/maintenance:usage-drift`
pub(crate) async fn usage_drift_route(State(state): State<Arc<FastcoreState>>) -> Response {
    let done = tokio::task::spawn_blocking(move || {
        let usage = crate::usage::maildir_usage(std::path::Path::new(&state.maildir_root))?;
        let mut conn = state
            .net_conn()
            .ok_or_else(|| std::io::Error::other("the network store is unreachable"))?;
        drift(usage, |account| {
            let key = mailrs_core_sidestate::keys::quota::used_key(account);
            let raw = conn.get(key.as_bytes())?;
            Ok(raw
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|s| s.trim().parse::<i64>().ok()))
        })
    })
    .await;
    match done {
        Ok(Ok(d)) => Json(d).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "usage drift failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_high_missing_and_equal_counters_are_told_apart() {
        let usage = vec![
            ("a@x.test".to_string(), 100),
            ("b@x.test".to_string(), 100),
            ("c@x.test".to_string(), 100),
            ("d@x.test".to_string(), 100),
            ("e@x.test".to_string(), 0),
        ];
        let counters = |a: &str| {
            Ok(match a {
                "a@x.test" => Some(100),
                "b@x.test" => Some(40),
                "c@x.test" => Some(250),
                _ => None,
            })
        };
        let d = drift(usage, counters).unwrap();
        assert_eq!((d.accounts, d.differing), (5, 4));
        assert_eq!(d.counter_low, ["b@x.test", "d@x.test"]);
        let diffs: Vec<Option<i64>> = d.rows.iter().map(|r| r.diff).collect();
        assert_eq!(diffs, [Some(0), Some(-60), Some(150), None, None]);
    }
}
