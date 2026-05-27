# mailrs-tls-rpt

[![Crates.io](https://img.shields.io/crates/v/mailrs-tls-rpt.svg)](https://crates.io/crates/mailrs-tls-rpt)
[![Docs.rs](https://docs.rs/mailrs-tls-rpt/badge.svg)](https://docs.rs/mailrs-tls-rpt)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

RFC 8460 **SMTP TLS Reporting (TLSRPT)** — `_smtp._tls.<domain>` TXT
record parser, the RFC 8460 §4 JSON report data model, the 14
`result-type` values from §4.3, and a builder that takes per-connection
event facts and emits an owned daily report ready to serialize, sign,
and submit.

**Pure: no SMTP, no HTTPS, no gzip.** Same shape as the rest of the
mailrs email-auth stones (`mailrs-spf`, `mailrs-dkim`, `mailrs-arc`,
`mailrs-mta-sts`). The caller does the network. We do the bytes.

## Why

MTA-STS (RFC 8461) and DANE (RFC 7672) tell receiving domains how to
enforce TLS for inbound mail. TLSRPT (RFC 8460) is the feedback
loop: senders that respect those policies can post daily reports
back to the receiving domain via `mailto:` or `https:` endpoints,
listing TLS attempts that succeeded and — more importantly —
TLS attempts that failed (with structured `result-type` codes).

The crate exists because the existing Rust ecosystem has parsers for
DMARC reports (a similar JSON-feedback concept) but nothing
purpose-built for TLSRPT. `mailrs-tls-rpt` provides:

- A TLSRPT record parser (`_smtp._tls.<domain>` TXT) → list of
  `RuaEndpoint::{Mailto, Https}`.
- A full RFC 8460 §4 JSON report data model with serde derive — the
  field names match the spec verbatim so `serde_json::to_string`
  produces wire-format reports.
- A `FailureType` enum covering all 14 §4.3 values
  (`starttls-not-supported`, `certificate-host-mismatch`, ...,
  `policy-not-published`).
- A `ReportBuilder` that takes per-connection event facts
  (`SuccessEvent` / `FailureEvent`) and aggregates them into
  buckets the way RFC 8460 §4.1 requires.

## Quick start

```rust
use mailrs_tls_rpt::{
    FailureEvent, FailureType, PolicyType, ReportBuilder, SuccessEvent,
};

let mut builder = ReportBuilder::new()
    .organization_name("GOLIA K.K.")
    .contact_info("mailto:tlsrpt@golia.jp")
    .report_id("golia-2026-05-23")
    .date_range("2026-05-23T00:00:00Z", "2026-05-24T00:00:00Z");

// Per-connection facts captured from your SMTP-client TLS results.
builder.record_success(SuccessEvent {
    policy_domain: "example.com".into(),
    policy_type: PolicyType::Sts,
    mx_host: "mail.example.com".into(),
});
builder.record_failure(FailureEvent {
    policy_domain: "example.com".into(),
    policy_type: PolicyType::Sts,
    mx_host: Some("mail.example.com".into()),
    result_type: FailureType::CertificateExpired,
    sending_mta_ip: Some("10.0.0.1".parse().unwrap()),
    receiving_ip: Some("203.0.113.5".parse().unwrap()),
    receiving_mx_helo: Some("mail.example.com".into()),
    additional_information: Some("cert NotAfter=2026-04-01".into()),
    failure_reason_code: Some("CERT_EXPIRED".into()),
});

let report = builder.build().unwrap();
let json = serde_json::to_vec(&report).unwrap();
// Now: gzip, attach to a TLSRPT email (Subject per RFC 8460 §5.3)
// OR POST to the rua https endpoint.
```

## What's in the box

| Module | Role |
|---|---|
| `record` | `_smtp._tls.<domain>` TXT parser. `v=TLSRPTv1; rua=mailto:…,https://…` → `Vec<RuaEndpoint>`. |
| `failure` | `FailureType` enum — 14 RFC 8460 §4.3 result-type strings, kebab-case serde. |
| `report` | Full §4 report model (`Report`, `PolicyReport`, `FailureDetail`, …) + `ReportBuilder`. |
| `error` | One `TlsRptError` enum for both parse paths. |

## What's *not* in the box (and won't be)

- **DNS lookup** of `_smtp._tls.<domain>` — bring your own resolver.
- **gzip compression** — feed the JSON bytes to `flate2` (or whichever
  gzip you already have) before attachment.
- **SMTP delivery** of the report email — your MTA already does this.
- **HTTPS POST** of the report — bring your own HTTP client.
- **Report signing** (DKIM over the TLSRPT email) — that's an
  `mailrs-dkim` job, not a TLSRPT one.

Forcing one choice on every caller is exactly the trade-off
`mailrs-tls-rpt` was extracted to avoid.

## Sister crates

| Crate | Role |
|---|---|
| `mailrs-mta-sts` | RFC 8461 — the policy side of inbound TLS |
| `mailrs-spf` / `mailrs-dkim` / `mailrs-dmarc` / `mailrs-arc` | The email-auth quartet (also pure parsers + decision logic) |
| `mailrs-tls-rpt` | **This crate.** The feedback side of MTA-STS / DANE |

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ❌ 2 errors, 0 warnings (`cargo doc --no-deps -p mailrs-tls-rpt`) |
| **test** | line cov: 95.9% (`cargo llvm-cov -p mailrs-tls-rpt --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 4 gate(s) `perf_gate.rs` |
| **size** | release rlib: 879 KB |
| **fuzz** | ✅ 2 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of **Apache License, Version 2.0**
([LICENSE-APACHE](./LICENSE-APACHE)) or **MIT License**
([LICENSE-MIT](./LICENSE-MIT)) at your option.

## Performance

Criterion benches: `cargo bench -p mailrs-tls-rpt`. Per-bench medians + regression budgets are documented in [`BUDGETS.md`](BUDGETS.md) (this crate) and the workspace [`PERFORMANCE.md`](../../PERFORMANCE.md).
