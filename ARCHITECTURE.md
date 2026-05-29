# Architecture

mailrs is a Cargo workspace of **44 crates** — **43 reusable library crates**
(42 of them published on [crates.io](https://crates.io/users/golia-jp)) plus
the `mailrs-server` binary that wires them into a deployable mail server.

The split is deliberate: anything that implements an RFC or a self-contained
concept lives in its own library crate — independently versioned, tested,
benchmarked, and publishable. The server binary holds only the
project-specific glue that turns those libraries into a running server.

## Design principles

- **One crate, one boundary.** Each library crate owns a single RFC or a
  single well-defined concept (`mailrs-spf` = RFC 7208, `mailrs-rate-limit` =
  token-bucket). Small surface, easy to reason about, reusable elsewhere.
- **Parsers do zero I/O.** Protocol and parser crates never touch the network
  or disk — all async I/O (Tokio) lives in the server binary. This keeps the
  libraries portable to any Rust project and trivial to unit-test.
- **Every published crate is self-contained.** README, CHANGELOG, criterion
  benches, a regression budget (`tests/perf_gate.rs`), `#![deny(missing_docs)]`,
  and a libFuzzer target wherever it parses untrusted input.

See [DEPS_AUDIT.md](./DEPS_AUDIT.md) for which external dependencies were
replaced by in-house crates, and [PERFORMANCE.md](./PERFORMANCE.md) for the
full benchmark ledger.

## Library crates

### Protocol parsers & formatters (zero-I/O)

| Crate | What it does |
|---|---|
| `mailrs-smtp-proto` | SMTP protocol parser, formatter, and session state machine (RFC 5321) |
| `mailrs-smtp-codec` | Tokio Decoder/Encoder for the RFC 5321 SMTP wire format |
| `mailrs-imap-proto` | IMAP4rev1 protocol parser, response formatter, and sequence-set helpers (RFC 3501/9051) |
| `mailrs-imap-codec` | Tokio Decoder/Encoder for the RFC 9051 IMAP wire format |
| `mailrs-imap-format` | IMAP wire-format helpers: FLAGS / INTERNALDATE / quoted strings / `BODY[…]` sections |
| `mailrs-rfc5322` | Pull-based RFC 5322 message parser — lazy header lookup, zero-alloc borrowed slices |
| `mailrs-rfc2047` | RFC 2047 MIME encoded-word decoder/encoder |
| `mailrs-rfc2231` | RFC 2231 MIME parameter encoder + decoder |
| `mailrs-mime` | RFC 2045/2046 MIME body-tree parser |
| `mailrs-mail-builder` | RFC 5322 / 2046 / 2047 / 2231 outbound mail builder |
| `mailrs-ical` | RFC 5545 iCalendar parser, serializer, and iTIP semantics (VTIMEZONE + RRULE) |
| `mailrs-jmap` | JMAP (RFC 8620 + 8621) server-side dispatcher — framework-agnostic, bring-your-own store |
| `mailrs-dav` | CalDAV (RFC 4791) + CardDAV (RFC 6352) server-side handlers, BYO data layer |
| `mailrs-sieve-core` | Native RFC 5228 Sieve interpreter — tokenizer + parser + evaluator |
| `mailrs-sieve` | Delivery-action wrapper over a Sieve engine (RFC 5228 email filtering) |

### Email authentication

| Crate | What it does |
|---|---|
| `mailrs-spf` | RFC 7208 Sender Policy Framework verifier |
| `mailrs-dkim` | RFC 6376 DKIM signature verifier + signer |
| `mailrs-dmarc` | RFC 7489 DMARC policy evaluation, alignment, and aggregate XML reporting |
| `mailrs-arc` | RFC 8617 Authenticated Received Chain — header parsing, chain extract, signature verify |
| `mailrs-srs` | Sender Rewriting Scheme — rewrite envelope From for SPF-aware forwarding, HMAC-keyed |

### Infrastructure & utilities

| Crate | What it does |
|---|---|
| `mailrs-rate-limit` | Token-bucket rate limiting trait + in-memory reference impl |
| `mailrs-auth-guard` | Per-IP / per-(IP, user) failed-auth counter with exponential-backoff lockout |
| `mailrs-dnsbl` | RFC 5782 DNS-based blocklist lookup |
| `mailrs-clamav` | Async ClamAV (clamd) TCP client |
| `mailrs-backoff` | Exponential backoff with optional jitter (AWS-style taxonomy) |
| `mailrs-webhook-signature` | HMAC-SHA256 webhook payload signing + verification |
| `mailrs-tls-reload` | Hot-reloadable rustls `ServerConfig` via arc-swap |
| `mailrs-acme` | ACME (RFC 8555 / Let's Encrypt) orchestration — HTTP-01 provisioning + renewal |
| `mailrs-dns` | Light hickory-resolver wrapper for the 5 DNS query types mail servers use (TXT/A/AAAA/MX/PTR) |
| `mailrs-mta-sts` | RFC 8461 MTA-STS — policy/record parsers, MX pattern matching, `enforce()` decision |
| `mailrs-tls-rpt` | RFC 8460 SMTP TLS Reporting — TXT parser, JSON report model, failure taxonomy |
| `mailrs-arf` | RFC 5965 Abuse Reporting Format (feedback-report) parser |

### Server building blocks (opinionated, bring-your-own-store)

| Crate | What it does |
|---|---|
| `mailrs-smtp-client` | Outbound SMTP client: MX resolution, DANE/STARTTLS, response parsing — async, transport-agnostic |
| `mailrs-maildir` | Maildir filesystem primitives: atomic delivery, directory scans, flag parsing |
| `mailrs-mailbox` | Mailbox metadata storage: `MailboxStore` trait + PostgreSQL reference impl |
| `mailrs-outbound-queue` | Outbound queue: DKIM signing, DSN generation, MTA-STS lookup, retry/backoff, pluggable store |
| `mailrs-delivery-executor` | Group-commit delivery executor on top of `mailrs-maildir` batch delivery |
| `mailrs-inbound` | Composable SMTP receive pipeline — Stage trait + early-reject executor + RFC 8601 results |
| `mailrs-shield` | SMTP anti-spam: DNSBL queries, greylisting (optional Redis), PTR/FCrDNS |
| `mailrs-postmaster` | Email-domain DNS health checks: MX, SPF, DKIM, DMARC, MTA-STS, TLS-RPT, BIMI, DANE, PTR |
| `mailrs-intelligence` | LLM-powered email analysis: extraction, importance scoring, spam classification, embeddings |
| `mailrs-clean` | Email content cleanup: HTML sanitization, tracking-pixel detection, quoted-reply splitting |
| `mailrs-attachment-extract` | Extract text from email attachments (PDF + image OCR) |

## The server binary (`crates/server/src/`)

`mailrs-server` owns all async I/O and wires the libraries together:

- **smtp_session / imap_session** — protocol handlers driving the parser crates
- **inbound/pipeline** — multi-stage acceptance: rate limit → PTR → DNSBL → greylist → SPF/DKIM/DMARC → content scan → Sieve → delivery
- **web** — REST API + WebSocket endpoints (Axum)
- **config** — all configuration via `MAILRS_*` environment variables
- **domain_store** — domain/account/alias resolution (PostgreSQL + Valkey + in-process cache)
- **event_bus** — Tokio broadcast channel connecting SMTP / IMAP / web in real time
- **users** — auth via PostgreSQL accounts (Argon2), with LDAP fallback
- **acme / tls** — Let's Encrypt automation + hot-reloadable TLS certificates

## Conventions

- Workspace dependencies are declared in the root `Cargo.toml` and referenced with `workspace = true`.
- Crate names are prefixed `mailrs-`; directory names are not (`crates/smtp-proto` → `mailrs-smtp-proto`).
- Configuration is environment-variable driven (`MAILRS_*`).
- PostgreSQL and Valkey are optional — the server starts in degraded mode if either is unavailable.
