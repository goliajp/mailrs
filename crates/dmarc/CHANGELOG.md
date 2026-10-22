# Changelog

All notable changes to `mailrs-dmarc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [3.0.0] - 2026-10-22

### Removed

- **Breaking:** `PgDmarcStore`, `BackendPool` and the `pg-store` / `spg`
  features. The crate no longer depends on `sqlx`. Migration: implement
  `DmarcStore` over your database (2.x's `PgDmarcStore` is a 60-line
  reference), or use `InMemoryDmarcStore`.

### Changed

- **Breaking:** `DmarcStore` takes its dates from the caller and never
  reads the clock: `record_result(report_date: NaiveDate, record)`,
  `get_results_for_date(date: NaiveDate)`, and `cleanup_old(days)` is
  replaced by `cleanup_before(cutoff: NaiveDate)` (results on `cutoff`
  stay). Before, the day a result belonged to was whatever the database
  used as its default, which a non-SQL store could not reproduce.
- The trait lives in the new `store` module; `mailrs_dmarc::DmarcStore`
  still resolves.

### Added

- `InMemoryDmarcStore`, a reference `DmarcStore`.
- `store::check_dmarc_store`, an executable contract for `DmarcStore`
  implementations.

## [2.1.1] - 2026-10-22

### Changed

- Dependencies: `quick-xml` 0.41 → 0.42, `zip` 8.6 → 9.0. Neither appears
  in the public API. No behaviour change.

## [2.1.0] - 2026-08-04

### Added

- `ingest` module for inbound aggregate reports (RFC 7489 §7.2):
  `extract_report_payloads(raw)` pulls the XML out of a report email
  (plain, gzip or zip attachment), and
  `parse_aggregate_report(xml)` returns an `AggregateReport`
  (`ReportMetadata`, `PolicyPublished`, `ReportRecord` rows with
  `Row`, `Identifiers` and `AuthResults`), with `total_count()` /
  `passing_count()` helpers. Inputs are bounded by `MAX_PAYLOAD_BYTES`
  and `MAX_RECORDS`; failures are reported as `IngestError`.
- `spg` feature: builds `PgDmarcStore` on spg-embedded instead of
  PostgreSQL. `BackendPool` names the pool type of the active backend.

### Changed

- `PgDmarcStore::new` takes `BackendPool` (equal to `sqlx::PgPool`
  unless the `spg` feature is enabled).
- XML report generation writes into one buffer with `write!` and is
  roughly 1.7-1.9 times faster. Output is unchanged.
- New dependencies: `mailrs-mime`, `quick-xml`, `serde`, `zip`.

## [2.0.0] - 2026-05-29

### Changed (breaking)

- `DkimSignatureResult::d_domain`, `SpfResult::domain`,
  `DmarcInput::from_domain` and `DmarcInput::policy_domain` are
  `compact_str::CompactString` instead of `String`. Build them with
  `.into()` from a `&str`.

### Changed

- `format_report_email` builds the report message with
  `mailrs-mail-builder` instead of a hand-written template. The gzipped
  XML is still a base64 attachment; the MIME boundary is now random per
  call.
- New dependencies: `compact_str`, `mailrs-mail-builder`.

## [1.1.0] - 2026-05-23

### Added

**Full DMARC policy evaluation (the mail-auth replacement).** Three new
modules complete the DMARC half of `stalwart/mail-auth`:

- **`policy`** — [`DmarcPolicy::parse`] for `v=DMARC1; p=...; sp=...;
  adkim=...; aspf=...; pct=...; rua=...; ruf=...` records (RFC 7489
  §6.3). 12 inline tests cover required-tag rejection, sp-inherits-p,
  whitespace tolerance, comma-separated rua lists, unknown-tag
  forward-compat.

- **`align`** — [`check`] implements RFC 7489 §3.1 identifier alignment
  for both DKIM (`d=` vs `From:`) and SPF (MAIL FROM vs `From:`), in
  both strict and relaxed modes. Relaxed uses the **Public Suffix List**
  via the `psl` crate (compile-time, no DNS) to extract organizational
  domains — so `mail.example.com` aligns with `example.com`, and
  `news.example.co.uk` aligns with `www.example.co.uk`. 14 inline tests.

- **`eval`** — [`evaluate`] is a pure function: SPF result + DKIM
  signature list + parsed policy → [`DmarcOutcome`] with
  `aligned_spf_pass` / `aligned_dkim_pass` / `dmarc_pass` /
  `disposition`. No DNS, no clock, no RNG — `pct=` sampling is up
  to the caller. Subdomain detection picks `sp=` over `p=` when the
  From: domain ≠ the policy domain. 17 inline tests.

### Dependencies

- Added `psl = "2"` (compile-time embedded Public Suffix List).
- Added `thiserror = "2"`.

### Tests

- Crate test count: 53 → 96 (+43).

### Notes

- This is the last piece of replacing `mail-auth` (the others were
  `mailrs-spf` and `mailrs-dkim`). With 1.1.0, `mailrs-dmarc` +
  `mailrs-spf` + `mailrs-dkim` together cover everything mail-auth
  shipped for inbound verification.
- The existing 1.0.x reporting API (`generate_dmarc_report_xml`,
  `format_report_email`, `DmarcStore` trait, Postgres reference impl,
  `extract_rua_from_dmarc_record`) is fully preserved.

## [1.0.2] - 2026-05-22

### Added
- `DmarcResultRecord` now derives `Default` and exposes a fluent `pub fn new(...)` constructor. Both are additive; existing struct-literal call sites keep working.
- Rustdoc on `DmarcResultRecord` with a six-argument constructor example and a partial-construction example via `Default::default()`.

### Notes
- `#[non_exhaustive]` was deliberately not added — it would be a breaking change for external struct-literal users. A future 2.0 will tighten the type with `#[non_exhaustive]` plus a builder.

## [1.0.1] - 2026-05-21

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.0] - 2026-05-20

### Added
- Initial release. DMARC (RFC 7489) aggregate report generation: result recording, XML report builder, report-mail formatter, and `rua` extraction. Pluggable store trait with a PostgreSQL reference implementation.
