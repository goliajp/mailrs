# mailrs-dmarc

[![Crates.io](https://img.shields.io/crates/v/mailrs-dmarc?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dmarc)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dmarc?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dmarc)
[![License](https://img.shields.io/crates/l/mailrs-dmarc?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-dmarc?style=flat-square)](https://crates.io/crates/mailrs-dmarc)

DMARC (RFC 7489) aggregate report tooling for Rust — result recording, XML report generation, report-mail formatting, `rua` extraction. Fills the gap [`mail-auth`] leaves on the **receiving / aggregating** side.

Extracted from [mailrs] so any Rust SMTP server can produce the daily aggregate reports their reporting partners expect.

## What it covers

`mail-auth` (excellent) handles DMARC **verification** — pulling the policy, evaluating SPF/DKIM alignment, deciding pass/fail. This crate covers what comes after:

| Step | mailrs-dmarc | mail-auth |
|---|---|---|
| Parse `_dmarc.<domain>` policy | — | ✓ |
| Verify SPF + DKIM + alignment | — | ✓ |
| **Record per-message results** | ✓ ([`DmarcStore`]) | — |
| **Generate aggregate XML** (RFC 7489 §12.4) | ✓ ([`generate_dmarc_report_xml`]) | — |
| **Format report mail** (multipart + gzip + base64) | ✓ ([`format_report_email`]) | — |
| **Extract `rua` mailbox** | ✓ ([`extract_rua_from_dmarc_record`]) | — |

## Quick start

```rust,no_run
use std::sync::Arc;
use mailrs_dmarc::{
    DmarcResultRecord, DmarcStore, PgDmarcStore,
    extract_rua_from_dmarc_record, format_report_email, generate_dmarc_report_xml,
};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let pool = sqlx::PgPool::connect("postgres://localhost/mail").await?;
let store: Arc<PgDmarcStore> = Arc::new(PgDmarcStore::new(pool));

// Inside your inbound pipeline, per-message:
store.record_result(&DmarcResultRecord {
    source_ip: "192.0.2.1".into(),
    from_domain: "sender.example".into(),
    spf_result: "pass".into(),
    dkim_result: "pass".into(),
    dmarc_result: "pass".into(),
    disposition: "none".into(),
}).await?;

// Daily aggregate report:
let results = store.get_results_for_date("2026-05-19").await?;
let xml = generate_dmarc_report_xml(
    "Reporter Org", "postmaster@reporter.example",
    "reporter.example!sender.example!2026-05-19",
    "sender.example", 1715990400, 1716076800, &results,
);
let rua = extract_rua_from_dmarc_record("v=DMARC1; p=quarantine; rua=mailto:dmarc@sender.example");
let email = format_report_email(
    "postmaster@reporter.example",
    rua.as_deref().unwrap_or("dmarc@sender.example"),
    "sender.example",
    "reporter.example!sender.example!2026-05-19",
    "2026-05-19", &xml,
);
// `email` is the raw multipart/mixed RFC 5322 message — hand it to your outbound queue.
# Ok(())
# }
```

## Bring your own store

The `pg-store` feature gives you `PgDmarcStore` (the reference impl). Disable it (`default-features = false`) and implement [`DmarcStore`] yourself:

```rust
use async_trait::async_trait;
use mailrs_dmarc::{DmarcResultRecord, DmarcStore};

struct FileStore { /* ... */ }

#[async_trait]
impl DmarcStore for FileStore {
    type Error = std::io::Error;

    async fn record_result(&self, r: &DmarcResultRecord) -> Result<(), std::io::Error> {
        // append-only log, kafka topic, S3 prefix — your call.
        # let _ = r; Ok(())
    }
    async fn get_results_for_date(&self, _date: &str) -> Result<Vec<DmarcResultRecord>, std::io::Error> {
        # Ok(vec![])
    }
    async fn cleanup_old(&self, _days: i64) -> Result<u64, std::io::Error> {
        # Ok(0)
    }
}
```

## Feature flags

| Flag | Default | What it enables |
|---|---|---|
| `pg-store` | yes | `PgDmarcStore` (sqlx + Postgres reference impl) |

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-dmarc`) |
| **test** | line cov: 96.0% (`cargo llvm-cov -p mailrs-dmarc --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 2 gate(s) `perf_gate.rs` |
| **size** | release rlib: 3.2 MB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`mail-auth`]: https://crates.io/crates/mail-auth

## Performance

Criterion benches: `cargo bench -p mailrs-dmarc`. Per-bench medians + regression budgets are documented in [`BUDGETS.md`](BUDGETS.md) (this crate) and the workspace [`PERFORMANCE.md`](../../PERFORMANCE.md).
