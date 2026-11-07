//! The content security policy reports webapi counted, over the last
//! seven days and since counting began, computed from its heartbeats.
//!
//! A heartbeat carries counts since that webapi start. A window's count
//! for one start is its last heartbeat in the window minus its last one
//! before the window (zero when the start began inside it); the window's
//! total is the sum over starts. Reports in the last minute before a
//! process stopped never reached a heartbeat and are not counted.

use std::collections::BTreeMap;

use mailrs_ops_report::wire::{CspEntry, WebapiBeat};
use serde::Serialize;

use crate::store::{StartCounts, Store};

/// The window the decision to enforce is taken over.
pub const WINDOW_SECS: i64 = 7 * 86_400;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CspSummary {
    /// When the earliest kept webapi heartbeat with counts arrived; `None`
    /// before the first one, and then both totals are `None` too.
    pub counting_since: Option<i64>,
    pub last_7_days: Option<CspTotals>,
    /// Since `counting_since`. Heartbeats are kept 14 days, so after that
    /// this covers the kept ones.
    pub since_counting: Option<CspTotals>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CspTotals {
    /// Unix seconds the window opens; the counting start for the second.
    pub from: i64,
    pub to: i64,
    pub reports: u64,
    pub by_directive: BTreeMap<String, u64>,
    /// Most reported first.
    pub distinct: Vec<CspEntry>,
    pub distinct_dropped: u64,
    pub pages_served: BTreeMap<String, u64>,
}

pub fn summary(store: &Store, now: i64) -> crate::store::Result<CspSummary> {
    let Some(since) = store.webapi_counting_since()? else {
        return Ok(CspSummary {
            counting_since: None,
            last_7_days: None,
            since_counting: None,
        });
    };
    let from = now - WINDOW_SECS;
    Ok(CspSummary {
        counting_since: Some(since),
        last_7_days: Some(totals(&store.webapi_starts_in(Some(from), now)?, from, now)),
        since_counting: Some(totals(&store.webapi_starts_in(None, now)?, since, now)),
    })
}

/// The sum over starts of what each counted inside the window.
pub fn totals(starts: &[StartCounts], from: i64, to: i64) -> CspTotals {
    let mut t = CspTotals {
        from,
        to,
        ..CspTotals::default()
    };
    let mut distinct: BTreeMap<(String, String, String, String, String), CspEntry> =
        BTreeMap::new();
    for s in starts {
        let zero = WebapiBeat::default();
        let before = s.before.as_ref().unwrap_or(&zero);
        let now = &s.inside;
        t.reports += now.csp_reports.saturating_sub(before.csp_reports);
        t.distinct_dropped += now
            .csp_distinct_dropped
            .saturating_sub(before.csp_distinct_dropped);
        add_map(
            &mut t.by_directive,
            &now.csp_by_directive,
            &before.csp_by_directive,
        );
        add_map(&mut t.pages_served, &now.pages_served, &before.pages_served);
        for e in &now.csp_distinct {
            let was = before
                .csp_distinct
                .iter()
                .find(|b| b.key() == e.key())
                .map_or(0, |b| b.count);
            let count = e.count.saturating_sub(was);
            if count == 0 {
                continue;
            }
            let (d, v, b, p, c) = e.key();
            let key = (d.into(), v.into(), b.into(), p.into(), c.into());
            match distinct.get_mut(&key) {
                Some(m) => {
                    m.count += count;
                    m.first_at = m.first_at.min(e.first_at);
                    m.last_at = m.last_at.max(e.last_at);
                }
                None => {
                    distinct.insert(key, CspEntry { count, ..e.clone() });
                }
            }
        }
    }
    t.distinct = distinct.into_values().collect();
    t.distinct
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.key().cmp(&b.key())));
    t
}

fn add_map(
    into: &mut BTreeMap<String, u64>,
    now: &BTreeMap<String, u64>,
    before: &BTreeMap<String, u64>,
) {
    for (k, n) in now {
        let d = n.saturating_sub(before.get(k).copied().unwrap_or(0));
        if d > 0 {
            *into.entry(k.clone()).or_default() += d;
        }
    }
}

#[cfg(test)]
mod tests;
