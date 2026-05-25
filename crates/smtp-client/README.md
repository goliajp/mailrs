# mailrs-smtp-client

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-client?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-client)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-client?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-client)
[![License](https://img.shields.io/crates/l/mailrs-smtp-client?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-client?style=flat-square)](https://crates.io/crates/mailrs-smtp-client)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Outbound SMTP client primitives for Rust — MX resolution, DANE/STARTTLS, multi-line response parsing.

Built on `tokio` + `rustls` + `hickory-resolver`. Implements the pieces an MTA actually needs to deliver mail safely across the public Internet: looking up MX records, picking the right server, opening a TLS connection that can be verified against DNSSEC-anchored TLSA records ([RFC 7672] DANE), and reading SMTP replies that wrap across multiple lines.

## Highlights

- **MX lookup with caching** — `resolve_mx()` returns the preference-sorted list; `MxCache` lets you reuse results across deliveries.
- **DANE verification** — `resolve_tlsa()` + `DaneVerifier` enforce TLSA-bound certificates on the SMTP relay, defending against active MITM that downgrades STARTTLS.
- **Connection driver** — `SmtpConnection` wraps the read/write loop with configurable per-command timeouts (`TimeoutConfig`) so a slow server can never wedge the sender.
- **Response parser** — `parse_response()` handles the `250-...` / `250 ...` multi-line format defined in [RFC 5321 §4.2.1].
- **dot-stuffing** — `dot_stuff()` escapes leading dots in the DATA payload so the message can never be truncated by a stray `\r\n.\r\n`.
- **Battle-tested** — extracted from [mailrs], a production Rust mail server.

## Quick start

```rust,no_run
use mailrs_smtp_client::{MxCache, SmtpConnection, TokioResolver, sort_mx_records};
use std::time::Duration;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let cache = MxCache::new(Duration::from_secs(300)); // cache MX answers for 5 minutes

// 1. resolve and preference-sort MX records for the recipient domain
let mut records = cache.resolve(&resolver, "example.com").await?;
sort_mx_records(&mut records);
let primary = records.first().ok_or("no MX")?;

// 2. connect (reads the banner internally) and EHLO
let mut conn = SmtpConnection::connect(&primary.exchange, 25).await?;
conn.ehlo("client.example.org").await?;

// 3. upgrade to TLS, re-EHLO, and hand off the message
let mut conn = conn.starttls(&primary.exchange).await?;
conn.ehlo("client.example.org").await?;
conn.deliver(
    "sender@example.org",
    &["bob@example.com"],
    b"Subject: hi\r\n\r\nhello\r\n",
).await?;
conn.quit().await?;
# Ok(()) }
```

See [`examples/resolve_and_connect.rs`](examples/resolve_and_connect.rs) for an end-to-end MX lookup + EHLO + QUIT walk-through.

## What this crate does NOT do

- No DKIM signing, no SPF check, no DMARC alignment. Those belong upstream (e.g. [mail-auth]) — this crate is the wire-level client only.
- No outbound queue, no retry logic, no DSN generation. See `mailrs-outbound-queue` for those.
- No SMTP server. See [`mailrs-smtp-proto`] for the receive-side state machine.

## Module overview

| Module | What it does |
|--------|--------------|
| `mx` | DNS MX lookup, preference-sort, in-memory cache, domain fallback. |
| `dane` | TLSA record resolution + cert verification per [RFC 7672]. |
| `connection` | `SmtpConnection` wraps a TLS-upgradable read/write loop with timeouts. |
| `response` | Parses single- and multi-line SMTP replies ([RFC 5321 §4.2.1]). |

## Performance

Measured with criterion 0.8 on Apple Silicon (M-series), `cargo bench`, release profile. Medians from 100-sample runs.

| Operation | Median | Notes |
|---|---|---|
| `parse_response("250 OK\r\n")` | ~30 ns | single-line reply |
| `parse_response(<10-line EHLO greeting>)` | ~290 ns | full multi-line continuation parsing |
| `dot_stuff(<5 KB body, no dots>)` | ~1.4 µs | scans for leading dots, returns input slice when none |
| `dot_stuff(<5 KB body, every-other line starts with .>)` | ~1.6 µs | copies on first hit, then dot-stuffs |
| `sort_mx_records(n=20)` | ~12 ns | preference + tie-break alphabetical |
| `fallback_to_domain("example.com")` | ~24 ns | constructs the single-record fallback |
| `MxCache::is_empty()` / `len()` / `cleanup()` (empty cache) | ~4 ns | mutex acquire + check |

`MxCache::resolve` itself isn't bench-able offline (it depends on a live `DnsResolver`); in production the hit path is one mutex-locked HashMap lookup + a `Vec<MxRecord>` clone.

Re-run locally with `cargo bench -p mailrs-smtp-client`. See [`tests/perf_gate.rs`](tests/perf_gate.rs) for the regression budgets.

## Why a separate crate?

Most "SMTP client" crates either bundle a full MUA (auth, MIME building, attachments) or stop at "TCP+EHLO+MAIL FROM". For an MTA you want neither: you already have the message bytes, and what you actually need is the long tail — preference-sorted MX lists, DANE-verified TLS, robust multi-line reply parsing, and timeouts that don't let one slow remote hold up a connection pool. That's what this crate is.

It's the outbound side of the [mailrs] mail server and is published independently so anyone building an MTA, a delivery-test harness, or a bounce probe in Rust can lean on the same battle-tested pieces.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-smtp-client`) |
| **test** | line cov: 72.9% (`cargo llvm-cov -p mailrs-smtp-client --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 3 gate(s) `perf_gate.rs` |
| **size** | release rlib: 1007 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[RFC 5321 §4.2.1]: https://datatracker.ietf.org/doc/html/rfc5321#section-4.2.1
[RFC 7672]: https://datatracker.ietf.org/doc/html/rfc7672
[mail-auth]: https://crates.io/crates/mail-auth
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[mailrs]: https://github.com/goliajp/mailrs
