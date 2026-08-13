//! `/api/admin/dmarc/*` — read side for inbound DMARC aggregate reports.
//!
//! Reports are written by `mailrs-fastcore`'s `dmarc_ingest` module into
//! the shared network kevy. This module only reads them.
//!
//! Key layout is duplicated here as constants rather than shared through
//! a crate boundary, matching the existing convention in
//! `receiver/src/kevy_dmarc.rs`. The authoritative definition —
//! including the `<sid>` construction rule — lives in
//! `crates/fastcore/src/dmarc_ingest.rs`; the two must move together.
//!
//! Per-source aggregates are computed on read rather than stored. They
//! are a derivation (`common/data-architecture.md`), the volume is a few
//! reports per day, and a stored copy could drift from the rows it
//! summarises.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::{require_permission, with_kevy};

/// Time-ordered index of stored reports. Mirrors
/// `fastcore::dmarc_ingest::INDEX_KEY`.
const INDEX_KEY: &[u8] = b"mailrs:dmarcrpt:index";
/// Per-report metadata hash prefix.
const REPORT_PREFIX: &str = "mailrs:dmarcrpt:report:";
/// Per-report row zset prefix.
const ROWS_PREFIX: &str = "mailrs:dmarcrpt:rows:";

/// Permission gating every route here. DMARC posture is domain config.
const PERMISSION: &str = "admin.domains";

/// Largest page a caller can request.
const MAX_LIMIT: usize = 200;
/// Default page size.
const DEFAULT_LIMIT: usize = 50;
/// Default lookback for the sources rollup.
const DEFAULT_DAYS: i64 = 30;

/// Query for [`list_reports`].
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Page size, clamped to [`MAX_LIMIT`].
    pub limit: Option<usize>,
}

/// Query for [`list_sources`].
#[derive(Debug, Deserialize)]
pub struct SourcesQuery {
    /// Restrict to one policy domain. Omit for all domains.
    pub domain: Option<String>,
    /// Lookback window in days. Defaults to [`DEFAULT_DAYS`].
    pub days: Option<i64>,
}

/// Report summary as returned by the list endpoint.
#[derive(Debug, Serialize)]
pub struct ReportSummary {
    /// Storage id, `<org_name>!<report_id>`.
    pub sid: String,
    /// Reporting organization.
    pub org_name: String,
    /// Contact address for the reporter.
    pub email: String,
    /// Domain the policy applies to.
    pub policy_domain: String,
    /// Window start, unix seconds.
    pub begin: i64,
    /// Window end, unix seconds.
    pub end: i64,
    /// Published policy at report time.
    pub p: String,
    /// Messages covered.
    pub total: u64,
    /// Messages that passed DMARC.
    pub passing: u64,
    /// Row count in the report.
    pub rows: u64,
}

/// Envelope for the list endpoint.
#[derive(Debug, Serialize)]
pub struct ReportListResponse {
    /// Reports, newest window first.
    pub items: Vec<ReportSummary>,
}

/// One row of a report, as stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRow {
    /// Sending IP.
    pub source_ip: String,
    /// Messages from this source in this group.
    pub count: u64,
    /// Disposition the reporter applied.
    pub disposition: String,
    /// Aligned DKIM outcome.
    pub dkim: String,
    /// Aligned SPF outcome.
    pub spf: String,
    /// RFC 5322 From domain.
    pub header_from: String,
    /// RFC 5321 MAIL FROM domain, when reported.
    pub envelope_from: Option<String>,
    /// Whether this group passed DMARC.
    pub passed: bool,
}

/// Detail response: summary plus rows.
#[derive(Debug, Serialize)]
pub struct ReportDetailResponse {
    /// The report's summary fields.
    pub report: ReportSummary,
    /// Every row, highest count first.
    pub rows: Vec<ReportRow>,
}

/// A sending source rolled up across reports.
#[derive(Debug, Serialize)]
pub struct SourceSummary {
    /// Sending IP.
    pub source_ip: String,
    /// Total messages seen from this source.
    pub total: u64,
    /// Messages that passed DMARC.
    pub passing: u64,
    /// Distinct policy domains this source sent as.
    pub domains: Vec<String>,
}

/// Envelope for the sources endpoint.
#[derive(Debug, Serialize)]
pub struct SourceListResponse {
    /// Sources, highest volume first.
    pub items: Vec<SourceSummary>,
    /// Messages covered by the window.
    pub total: u64,
    /// Messages that passed DMARC in the window.
    pub passing: u64,
    /// Reports the rollup was computed from.
    pub reports: usize,
}

/// GET /api/admin/dmarc/reports — newest reports first.
pub async fn list_reports(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ReportListResponse>, StatusCode> {
    require_permission(&state, &user, PERMISSION).await?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let raw = with_kevy(move |c| {
        // kevy-client exposes only ascending `zrange`, and without
        // scores. Take the tail (highest window-begin) and reverse to
        // get newest-first.
        let card = c.zcard(INDEX_KEY)?;
        let start = card.saturating_sub(limit) as i64;
        let ids = c.zrange(INDEX_KEY, start, -1)?;
        let mut out = Vec::with_capacity(ids.len());
        for sid_bytes in ids.into_iter().rev() {
            let sid = String::from_utf8_lossy(&sid_bytes).into_owned();
            let flat = c.hgetall(format!("{REPORT_PREFIX}{sid}").as_bytes())?;
            out.push((sid, flat));
        }
        Ok(out)
    })?;

    let items = raw
        .into_iter()
        .filter_map(|(sid, flat)| summary_from_hash(&sid, &flat))
        .collect();
    Ok(Json(ReportListResponse { items }))
}

/// GET /api/admin/dmarc/reports/{sid} — one report with its rows.
pub async fn get_report(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(sid): Path<String>,
) -> Result<Json<ReportDetailResponse>, StatusCode> {
    require_permission(&state, &user, PERMISSION).await?;

    let key_sid = sid.clone();
    let (flat, row_blobs) = with_kevy(move |c| {
        let flat = c.hgetall(format!("{REPORT_PREFIX}{key_sid}").as_bytes())?;
        let rows = c.zrange(format!("{ROWS_PREFIX}{key_sid}").as_bytes(), 0, -1)?;
        Ok((flat, rows))
    })?;

    let report = summary_from_hash(&sid, &flat).ok_or(StatusCode::NOT_FOUND)?;
    let mut rows: Vec<ReportRow> = row_blobs
        .into_iter()
        .filter_map(|blob| serde_json::from_slice::<ReportRow>(&blob).ok())
        .collect();
    // Highest-volume sources first. The zset is scored by count, but
    // kevy-client's zrange does not return scores, so sort here.
    rows.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.source_ip.cmp(&b.source_ip))
    });
    Ok(Json(ReportDetailResponse { report, rows }))
}

/// GET /api/admin/dmarc/sources — per-source-IP rollup over a window.
pub async fn list_sources(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<SourcesQuery>,
) -> Result<Json<SourceListResponse>, StatusCode> {
    require_permission(&state, &user, PERMISSION).await?;
    let days = q.days.unwrap_or(DEFAULT_DAYS).clamp(1, 365);
    let cutoff = now_secs() - days * 86_400;

    let raw = with_kevy(move |c| {
        let card = c.zcard(INDEX_KEY)?;
        let start = card.saturating_sub(MAX_LIMIT) as i64;
        let ids = c.zrange(INDEX_KEY, start, -1)?;
        let mut out = Vec::new();
        for sid_bytes in ids.into_iter().rev() {
            let sid = String::from_utf8_lossy(&sid_bytes).into_owned();
            let flat = c.hgetall(format!("{REPORT_PREFIX}{sid}").as_bytes())?;
            let rows = c.zrange(format!("{ROWS_PREFIX}{sid}").as_bytes(), 0, -1)?;
            out.push((flat, rows));
        }
        Ok(out)
    })?;

    Ok(Json(rollup_sources(raw, q.domain.as_deref(), cutoff)))
}

/// One report as it comes back from kevy: the metadata hash flattened
/// to `[k, v, k, v, ...]`, paired with its row-zset members.
type RawReport = (Vec<Vec<u8>>, Vec<Vec<u8>>);

/// Fold report rows into a per-source rollup.
///
/// `cutoff` is a unix-second lower bound on the report's window start,
/// read from the stored `begin` field. It is not taken from the index
/// score because kevy-client's `zrange` does not return scores.
fn rollup_sources(
    raw: Vec<RawReport>,
    domain_filter: Option<&str>,
    cutoff: i64,
) -> SourceListResponse {
    let mut by_ip: BTreeMap<String, SourceSummary> = BTreeMap::new();
    let mut total = 0u64;
    let mut passing = 0u64;
    let mut reports = 0usize;

    for (flat, row_blobs) in raw {
        let fields = hash_to_map(&flat);
        if num(&fields, "begin") < cutoff {
            continue;
        }
        let policy_domain = fields.get("policy_domain").cloned().unwrap_or_default();
        if let Some(want) = domain_filter
            && !policy_domain.eq_ignore_ascii_case(want)
        {
            continue;
        }
        reports += 1;
        for blob in row_blobs {
            let Ok(row) = serde_json::from_slice::<ReportRow>(&blob) else {
                continue;
            };
            total += row.count;
            if row.passed {
                passing += row.count;
            }
            let entry = by_ip
                .entry(row.source_ip.clone())
                .or_insert_with(|| SourceSummary {
                    source_ip: row.source_ip.clone(),
                    total: 0,
                    passing: 0,
                    domains: Vec::new(),
                });
            entry.total += row.count;
            if row.passed {
                entry.passing += row.count;
            }
            if !policy_domain.is_empty() && !entry.domains.contains(&policy_domain) {
                entry.domains.push(policy_domain.clone());
            }
        }
    }

    let mut items: Vec<SourceSummary> = by_ip.into_values().collect();
    items.sort_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then_with(|| a.source_ip.cmp(&b.source_ip))
    });
    SourceListResponse {
        items,
        total,
        passing,
        reports,
    }
}

/// Turn kevy's flat `[k, v, k, v, ...]` hash reply into a map.
fn hash_to_map(flat: &[Vec<u8>]) -> BTreeMap<String, String> {
    flat.chunks_exact(2)
        .map(|kv| {
            (
                String::from_utf8_lossy(&kv[0]).into_owned(),
                String::from_utf8_lossy(&kv[1]).into_owned(),
            )
        })
        .collect()
}

/// Build a summary from a report hash. Returns `None` when the hash is
/// empty (unknown sid) or missing the fields that make it a report.
fn summary_from_hash(sid: &str, flat: &[Vec<u8>]) -> Option<ReportSummary> {
    let f = hash_to_map(flat);
    let org_name = f.get("org_name")?.clone();
    let policy_domain = f.get("policy_domain")?.clone();
    Some(ReportSummary {
        sid: sid.to_string(),
        org_name,
        email: f.get("email").cloned().unwrap_or_default(),
        policy_domain,
        begin: num(&f, "begin"),
        end: num(&f, "end"),
        p: f.get("p").cloned().unwrap_or_default(),
        total: num(&f, "total") as u64,
        passing: num(&f, "passing") as u64,
        rows: num(&f, "rows") as u64,
    })
}

/// Read a numeric hash field, defaulting to 0 when absent or unparseable.
fn num(f: &BTreeMap<String, String>, key: &str) -> i64 {
    f.get(key).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Current unix seconds.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(pairs: &[(&str, &str)]) -> Vec<Vec<u8>> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.as_bytes().to_vec(), v.as_bytes().to_vec()])
            .collect()
    }

    fn row(ip: &str, count: u64, passed: bool) -> Vec<u8> {
        serde_json::to_vec(&ReportRow {
            source_ip: ip.into(),
            count,
            disposition: "none".into(),
            dkim: "pass".into(),
            spf: "pass".into(),
            header_from: "golia.jp".into(),
            envelope_from: None,
            passed,
        })
        .expect("row serializes")
    }

    #[test]
    fn summary_needs_org_and_domain() {
        assert!(summary_from_hash("x", &[]).is_none());
        assert!(summary_from_hash("x", &hash(&[("org_name", "g")])).is_none());
        assert!(
            summary_from_hash("x", &hash(&[("org_name", "g"), ("policy_domain", "d")])).is_some()
        );
    }

    #[test]
    fn summary_parses_numeric_fields_and_tolerates_junk() {
        let h = hash(&[
            ("org_name", "google.com"),
            ("policy_domain", "golia.jp"),
            ("begin", "1784937600"),
            ("total", "7"),
            ("passing", "not-a-number"),
        ]);
        let s = summary_from_hash("google.com!1", &h).expect("summary");

        assert_eq!(s.begin, 1784937600);
        assert_eq!(s.total, 7);
        assert_eq!(s.passing, 0, "unparseable numerics read as zero, not error");
        assert_eq!(s.end, 0, "absent numerics read as zero");
    }

    /// No time filtering — every fixture report is in-window.
    const NO_CUTOFF: i64 = 0;

    #[test]
    fn rollup_totals_across_reports_and_sorts_by_volume() {
        let raw = vec![
            (
                hash(&[("org_name", "a"), ("policy_domain", "golia.jp")]),
                vec![row("203.0.113.1", 5, true), row("198.51.100.2", 9, false)],
            ),
            (
                hash(&[("org_name", "b"), ("policy_domain", "golia.jp")]),
                vec![row("203.0.113.1", 3, true)],
            ),
        ];

        let out = rollup_sources(raw, None, NO_CUTOFF);

        assert_eq!(out.reports, 2);
        assert_eq!(out.total, 17);
        assert_eq!(out.passing, 8);
        assert_eq!(out.items.len(), 2);
        assert_eq!(
            out.items[0].source_ip, "198.51.100.2",
            "highest volume first"
        );
        assert_eq!(out.items[1].source_ip, "203.0.113.1");
        assert_eq!(out.items[1].total, 8, "same IP summed across reports");
        assert_eq!(out.items[1].passing, 8);
    }

    #[test]
    fn rollup_honours_the_domain_filter() {
        let raw = vec![
            (
                hash(&[("org_name", "a"), ("policy_domain", "golia.jp")]),
                vec![row("203.0.113.1", 5, true)],
            ),
            (
                hash(&[("org_name", "b"), ("policy_domain", "golia.ai")]),
                vec![row("198.51.100.2", 9, true)],
            ),
        ];

        let out = rollup_sources(raw, Some("golia.jp"), NO_CUTOFF);

        assert_eq!(out.reports, 1);
        assert_eq!(out.total, 5);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].source_ip, "203.0.113.1");
    }

    #[test]
    fn rollup_drops_reports_older_than_the_cutoff() {
        let raw = vec![
            (
                hash(&[
                    ("org_name", "recent"),
                    ("policy_domain", "golia.jp"),
                    ("begin", "2000"),
                ]),
                vec![row("203.0.113.1", 5, true)],
            ),
            (
                hash(&[
                    ("org_name", "stale"),
                    ("policy_domain", "golia.jp"),
                    ("begin", "500"),
                ]),
                vec![row("198.51.100.2", 9, true)],
            ),
        ];

        let out = rollup_sources(raw, None, 1000);

        assert_eq!(out.reports, 1, "only the in-window report counts");
        assert_eq!(out.total, 5);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].source_ip, "203.0.113.1");
    }

    #[test]
    fn rollup_records_every_domain_a_source_sent_as() {
        let raw = vec![
            (
                hash(&[("org_name", "a"), ("policy_domain", "golia.jp")]),
                vec![row("203.0.113.1", 1, true)],
            ),
            (
                hash(&[("org_name", "a"), ("policy_domain", "golia.ai")]),
                vec![row("203.0.113.1", 1, true)],
            ),
        ];

        let out = rollup_sources(raw, None, NO_CUTOFF);
        assert_eq!(out.items[0].domains, vec!["golia.jp", "golia.ai"]);
    }

    #[test]
    fn rollup_skips_unparseable_rows_without_losing_the_report() {
        let raw = vec![(
            hash(&[("org_name", "a"), ("policy_domain", "golia.jp")]),
            vec![b"not json".to_vec(), row("203.0.113.1", 4, true)],
        )];

        let out = rollup_sources(raw, None, NO_CUTOFF);
        assert_eq!(out.total, 4);
        assert_eq!(out.items.len(), 1);
    }

    #[test]
    fn empty_input_rolls_up_to_zeroes() {
        let out = rollup_sources(Vec::new(), None, NO_CUTOFF);
        assert_eq!(out.reports, 0);
        assert_eq!(out.total, 0);
        assert!(out.items.is_empty());
    }

    // The three tests below pin the exact JSON these handlers emit.
    // `web/src/wire/schemas/__tests__/dmarc.test.ts` uses byte-identical
    // fixtures, so a field rename here fails on this side before it can
    // reach the browser as a silent validation error.

    #[test]
    fn report_list_wire_shape() {
        let resp = ReportListResponse {
            items: vec![ReportSummary {
                sid: "google.com!1234567890".into(),
                org_name: "google.com".into(),
                email: "noreply-dmarc-support@google.com".into(),
                policy_domain: "golia.jp".into(),
                begin: 1784937600,
                end: 1785023999,
                p: "quarantine".into(),
                total: 7,
                passing: 5,
                rows: 2,
            }],
        };

        assert_eq!(
            serde_json::to_value(&resp).expect("serializes"),
            serde_json::json!({
                "items": [{
                    "sid": "google.com!1234567890",
                    "org_name": "google.com",
                    "email": "noreply-dmarc-support@google.com",
                    "policy_domain": "golia.jp",
                    "begin": 1784937600,
                    "end": 1785023999,
                    "p": "quarantine",
                    "total": 7,
                    "passing": 5,
                    "rows": 2
                }]
            })
        );
    }

    #[test]
    fn report_row_wire_shape_keeps_null_envelope_from() {
        let row = ReportRow {
            source_ip: "203.0.113.10".into(),
            count: 5,
            disposition: "none".into(),
            dkim: "pass".into(),
            spf: "pass".into(),
            header_from: "golia.jp".into(),
            envelope_from: None,
            passed: true,
        };

        assert_eq!(
            serde_json::to_value(&row).expect("serializes"),
            serde_json::json!({
                "source_ip": "203.0.113.10",
                "count": 5,
                "disposition": "none",
                "dkim": "pass",
                "spf": "pass",
                "header_from": "golia.jp",
                "envelope_from": null,
                "passed": true
            }),
            "envelope_from must serialize as null, not be omitted — the \
             frontend schema types it as nullable, not optional"
        );
    }

    #[test]
    fn source_list_wire_shape() {
        let resp = SourceListResponse {
            items: vec![SourceSummary {
                source_ip: "203.0.113.10".into(),
                total: 8,
                passing: 8,
                domains: vec!["golia.jp".into()],
            }],
            total: 8,
            passing: 8,
            reports: 2,
        };

        assert_eq!(
            serde_json::to_value(&resp).expect("serializes"),
            serde_json::json!({
                "items": [{
                    "source_ip": "203.0.113.10",
                    "total": 8,
                    "passing": 8,
                    "domains": ["golia.jp"]
                }],
                "total": 8,
                "passing": 8,
                "reports": 2
            })
        );
    }
}
