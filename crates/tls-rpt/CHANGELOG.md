# Changelog — mailrs-tls-rpt

## 1.1.0 — 2026-05-23

### Added

- `pub mod submit` — pure helpers for packaging a [`Report`] into
  the bytes a TLSRPT receiver expects on either submission path:
  - `gzip_report(&Report) -> std::io::Result<Vec<u8>>` —
    `serde_json::to_vec` then gzip with default level. Body for
    HTTPS POST (`Content-Type: application/tlsrpt+gzip`) or for
    the email attachment.
  - `build_submission_email(&SubmissionEmailOpts) -> Vec<u8>` —
    assembles an RFC 5322 `multipart/report; report-type=tlsrpt`
    email body wrapping the gzipped report as an
    `application/tlsrpt+gzip` attachment, per RFC 8460 §5.3.
    Subject format exact:
    `Report Domain: <recv> Submitter: <us> Report-ID: <id@us>`.
    Caller prepends `DKIM-Signature:` and submits via SMTP.
  - `SubmissionEmailOpts { from_address, to_address,
    receiving_domain, submitter_domain, report_id, date_rfc2822,
    boundary, report_gzipped }`.
- New runtime deps: `flate2` (gzip), `base64` (MIME attachment).

### Tests

- 9 new tests covering gzip round-trip, RFC-8460 subject format,
  Message-ID shape, attachment Content-Type, supplied-boundary
  use, CRLF-only line endings, and RFC 2045 §6.8 76-char base64
  line wrapping.

### Why this release exists

mailrs-tls-rpt 1.0 shipped the data model + report builder. 1.1
adds the "package for the wire" layer so the server (or any
caller) can produce the actual bytes that go on the network. The
two submission transports (SMTP via `mailto:` rua endpoints, HTTPS
POST via `https:` rua endpoints) are out of scope — both are pure
caller concerns, since they need access to the caller's outbound
SMTP queue / HTTP client / DKIM key.

## 1.0.0 — 2026-05-23

Initial stable release. RFC 8460 SMTP TLS Reporting (TLSRPT) record
parser + JSON report data model + event-fact-aggregating builder.

### Parsers

- **TLSRPT TXT record** (`TlsRptRecord::parse`):
  `v=TLSRPTv1; rua=mailto:…,https://…`. Multiple comma-separated
  `rua` endpoints. Schemes `mailto:` (preserves local-part case per
  RFC 6068 §2) and `https:` (URI scheme case-insensitive); anything
  else is `InvalidEndpoint`. Forward-compatible: unknown tags
  ignored.

### Report data model (RFC 8460 §4)

Full structural coverage of the JSON wire format:

- `Report { organization_name, date_range, contact_info, report_id, policies }`
- `DateRange { start_datetime, end_datetime }` — RFC 3339 strings
  (caller formats — no chrono dep)
- `PolicyReport { policy, summary, failure_details }`
- `PolicyBlock { policy_type, policy_string, policy_domain, mx_host }`
- `PolicyType` — `sts` / `tlsa` / `no-policy-found`
- `SummaryBlock { total_successful_session_count, total_failure_session_count }`
- `FailureDetail` — every RFC 8460 §4.3 field, all optionals
  serialized via `skip_serializing_if = Option::is_none`
- `FailureType` enum — all 14 `result-type` values from §4.3
  (`starttls-not-supported`, `certificate-host-mismatch`, ...,
  `policy-not-published`), kebab-case via serde

### Builder

- `ReportBuilder::new()` → chainable setters for required header
  fields + `policy_string(domain, type, lines)` to attach the raw
  policy bytes.
- `record_success(SuccessEvent)` / `record_failure(FailureEvent)` —
  per-connection event facts. Buckets by `(policy_domain,
  policy_type)`; failures sub-bucket by full context tuple
  (result-type + IPs + MX + reason) so duplicate failures collapse
  to a single row with the count.
- `build()` → owned `Report` with stable bucket order (sorted by
  domain then policy-type) — deterministic output, easy to diff.

### Measured perf

Apple M-class silicon, release build, criterion microbenches:

| Operation                              | Time   |
|----------------------------------------|--------|
| `TlsRptRecord::parse` (1 rua)          | 164 ns |
| `TlsRptRecord::parse` (3 rua)          | 280 ns |
| `ReportBuilder::build` (100 successes) |  2.7 µs |
| `ReportBuilder::build` (100 mixed)     | 14.5 µs |
| `serde_json::to_vec` (100-success)     | 750 ns |

### Tests

26 inline unit tests + perf-gate integration tests + 2 fuzz targets
(record parser + report deserializer).

### Design

Pure: no SMTP, no HTTPS, no gzip, no DNS. Same shape as the rest of
the mailrs email-auth stones — the caller does the network. Only
runtime dependencies are `serde` + `serde_json` (and that's by
design — TLSRPT IS a JSON format).
