//! webapi's heartbeats, read back for the CSP report counts.

use mailrs_ops_report::wire::{Detail, Report, WebapiBeat};
use rusqlite::{OptionalExtension, params};

use super::{Result, Store};

/// One start of one webapi instance, as far as a window sees it: its last
/// counts before the window opened, and its last counts inside it. Counts
/// start at zero with every start, so a start is the unit to subtract in.
#[derive(Debug, Clone, PartialEq)]
pub struct StartCounts {
    pub before: Option<WebapiBeat>,
    pub inside: WebapiBeat,
}

impl Store {
    /// When the earliest kept webapi heartbeat with counts arrived.
    pub fn webapi_counting_since(&self) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT received_at FROM process_reports_events
                 WHERE process = 'webapi' AND kind = 'heartbeat'
                   AND json_extract(report, '$.detail.type') = 'webapi'
                 ORDER BY received_at LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every webapi start with a heartbeat received in `[from, to]`;
    /// `from = None` takes everything kept up to `to`.
    pub fn webapi_starts_in(&self, from: Option<i64>, to: i64) -> Result<Vec<StartCounts>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT MAX(CASE WHEN received_at < ?1 THEN id END),
                    MAX(CASE WHEN received_at >= ?1 AND received_at <= ?2 THEN id END)
             FROM process_reports_events
             WHERE process = 'webapi' AND kind = 'heartbeat'
             GROUP BY instance, start_count",
        )?;
        let ids = st
            .query_map(params![from.unwrap_or(i64::MIN), to], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut beat = c.prepare("SELECT report FROM process_reports_events WHERE id = ?1")?;
        let mut read = |id: i64| -> Result<Option<WebapiBeat>> {
            let json: String = beat.query_row([id], |r| r.get(0))?;
            let report: Report =
                serde_json::from_str(&json).map_err(|e| super::StoreError(e.to_string()))?;
            Ok(match report.detail {
                Some(Detail::Webapi(b)) => Some(b),
                _ => None,
            })
        };
        let mut out = Vec::new();
        for (before, inside) in ids {
            let Some(inside) = inside else { continue };
            let Some(inside) = read(inside)? else {
                continue;
            };
            let before = match before {
                Some(id) => read(id)?,
                None => None,
            };
            out.push(StartCounts { before, inside });
        }
        Ok(out)
    }
}
