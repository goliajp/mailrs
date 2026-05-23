# mailrs

An AI-native mail server written in Rust with a React web UI. Handles SMTP
inbound/outbound, IMAP, JMAP, CalDAV/CardDAV, ManageSieve, POP3, and provides
a modern conversational email interface.

## Architecture

mailrs is organized as a Cargo workspace with **32 crates** — 1 server
binary + **31 published libraries** ("stones") on [crates.io](https://crates.io/users/golia-jp).

The stones cover the full email-protocol + auth stack and are individually
reusable in any Rust project; the server binary is the "cement" that wires
them into a deployable mail server. See [ARCHITECTURE.md](./ARCHITECTURE.md)
for the stone-by-stone breakdown and [DEPS_AUDIT.md](./DEPS_AUDIT.md) for
the rewrite-vs-keep ledger on every external dependency.

```
server (mailrs-server binary, cement)
│
├── Protocol parsers (zero-I/O)
│   ├── smtp-proto / imap-proto         — wire-format command parsers + state machines
│   ├── rfc5322 / rfc2047 / rfc2231     — RFC 5322 headers, encoded-words, MIME params
│   ├── mime                            — RFC 2045/2046 MIME body tree
│   ├── ical                            — RFC 5545 iCalendar + iTIP
│   ├── jmap / dav                      — JMAP (RFC 8620), CalDAV/CardDAV (RFC 4791/6352)
│
├── Email auth (mail-auth full replacement)
│   ├── spf  / dkim  / dmarc            — RFC 7208 / 6376 / 7489
│   └── srs                             — Sender Rewriting Scheme for forwarding
│
├── Infra primitives
│   ├── rate-limit / auth-guard         — token bucket / per-IP lockout
│   ├── dnsbl / clamav                  — anti-spam + AV
│   ├── backoff / webhook-signature     — retry math + HMAC payload signing
│   ├── tls-reload / acme               — hot-reload rustls + Let's Encrypt
│   └── dns                             — minimal hickory wrapper
│
└── Server building blocks
    ├── smtp-client / outbound-queue    — outbound delivery
    ├── maildir / mailbox               — storage + metadata
    ├── inbound / shield                — multi-stage acceptance + anti-spam
    ├── postmaster                      — bounce/DSN + DNS health checks
    ├── intelligence / clean            — LLM classification + HTML sanitize
```

Protocol crates have **zero I/O** — the server crate owns all async
networking via Tokio. Every stone has its own perf budget
(`tests/perf_gate.rs`), criterion benches, fuzz target where applicable,
and a CHANGELOG.

### Performance posture (full ledger in [PERFORMANCE.md](./PERFORMANCE.md))

| Scenario | mailrs | competitor | gap |
|---|---:|---:|---|
| RFC 5322 message scan + Subject + From extract | 46 ns | Go `net/mail`: 1440 ns | **mailrs 31×** |
| iCalendar parse | 1.76 µs | C `libical` 4.0: 7.03 µs | **mailrs 4×** |
| SMTP `EHLO` command parse | 18 ns | Rust `smtp-codec`: 126 ns | **mailrs 7×** |
| DKIM-Signature realistic parse | 405 ns | Rust `mail-auth` 0.9: 423 ns | **mailrs +4%** |
| Rate-limit hot-key check | 13-16 ns | Rust `governor` 0.10: 14-18 ns | **mailrs / tied** |

Cross-language harness (with C / Go runners): `bench-harness/`.

### Quality posture

- **Tests**: ~1500 across the workspace; every published crate ≥ 30 tests/kloc.
- **Fuzz**: 13 libFuzzer targets across 8 parsers (`smtp-proto`, `imap-proto`, `rfc5322`, `rfc2047`, `mime`, `dkim`, `spf`, `ical`). **~120M iterations run cumulatively, 1 real bug caught + fixed** (`rfc2047::encode` roundtrip on inputs containing `=?`).
- **Property tests**: 9 proptest cases on rfc2047, rfc2231, srs encoder/decoder roundtrips.
- **Cross-competitor benches**: 8 head-to-head benches against `mail-auth` (×2), `mail-parser` (×3), `icalendar`, `governor`, `exponential-backoff`, `smtp-codec`, `imap-codec`.
- **Build hygiene**: `#[deny(warnings)]` + `#[deny(clippy::all)]` workspace-wide; `#![deny(missing_docs)]` on every published crate.
- **Cargo audit**: clean (637 deps, 0 vulns at last check).

### Inbound Pipeline

Incoming mail passes through a multi-stage acceptance pipeline: rate limiting, PTR check, DNSBL, greylisting, SPF/DKIM/DMARC verification, content scanning, Sieve filtering, and final delivery.

### Web Frontend

React 19 + TypeScript + Vite + Tailwind CSS 4 + Jotai for state management. Real-time updates via WebSocket. Conversation-style thread view with Markdown rendering.

## Tech Stack

| Layer       | Technology                                    |
|-------------|-----------------------------------------------|
| Server      | Rust, Tokio, Axum                             |
| Database    | PostgreSQL 18 (pgvector)                      |
| Cache       | Valkey / Redis                                |
| Storage     | Maildir                                       |
| Frontend    | React 19, Vite 7, Tailwind CSS 4, Bun         |
| Auth        | PostgreSQL accounts + Argon2, TOTP 2FA, LDAP  |
| TLS         | Let's Encrypt (ACME) with hot-reloadable certs|

## Features

- **SMTP** — Full inbound and outbound with STARTTLS, DKIM signing, SPF/DMARC verification
- **IMAP** — IMAP server with IDLE support for real-time notifications
- **POP3** — POP3 server for legacy client support
- **JMAP** — JMAP protocol support (RFC 8620)
- **CalDAV/CardDAV** — Calendar and contact sync for Thunderbird, Apple Calendar/Contacts
- **ManageSieve** — Remote Sieve script management (RFC 5804)
- **Web UI** — Conversational email interface with real-time WebSocket updates
- **MCP** — Model Context Protocol server with 52 tools for AI agent integration
- **Security** — Greylisting, DNSBL, rate limiting, MTA-STS, TOTP 2FA
- **Delivery** — Outbound queue with retry logic and DSN (bounce) generation
- **Sieve** — Server-side mail filtering
- **ACME** — Automatic TLS certificate provisioning via Let's Encrypt
- **AI** — Email classification, AI-assisted drafting and reply suggestions (Gemini)

## Quick Start

### Docker Compose (recommended)

```bash
git clone https://github.com/goliajp/mailrs.git
cd mailrs

# configure environment
export MAILRS_HOSTNAME=mx.example.com
export MAILRS_LOCAL_DOMAINS=example.com

# start all services
docker compose up -d
```

This starts PostgreSQL (with pgvector), Valkey, and the mailrs server. Ports exposed:

| Port | Service         |
|------|-----------------|
| 25   | SMTP            |
| 110  | POP3            |
| 143  | IMAP            |
| 465  | SMTPS           |
| 587  | SMTP Submission |
| 993  | IMAPS           |
| 995  | POP3S           |
| 3100 | Web UI / API    |
| 4190 | ManageSieve     |

### Local Development

Prerequisites: PostgreSQL and Valkey running locally.

```bash
# start dependencies
docker compose up postgres valkey -d

# run the full dev stack (SMTP + IMAP + Web API + Vite dev server)
./scripts/dev.sh
```

Or run components separately:

```bash
# rust server
cargo run --bin mailrs-server

# web frontend (requires Bun)
cd web && bun install && bun run dev
```

## Configuration

All configuration is via `MAILRS_*` environment variables. Key settings:

| Variable                | Description                        |
|-------------------------|------------------------------------|
| `MAILRS_HOSTNAME`       | Server hostname (MX record)        |
| `MAILRS_LOCAL_DOMAINS`  | Comma-separated local domains      |
| `MAILRS_PG_URL`         | PostgreSQL connection URL           |
| `MAILRS_VALKEY_URL`     | Valkey/Redis connection URL         |
| `MAILRS_MAILDIR`        | Maildir storage path               |
| `MAILRS_USERS_FILE`     | Path to `users.toml`               |
| `MAILRS_DKIM_*`         | DKIM signing configuration         |
| `MAILRS_ACME_*`         | Let's Encrypt ACME settings        |
| `MAILRS_TLS_CERT/KEY`   | TLS certificate and key paths      |

PostgreSQL and Valkey are optional — the server starts in degraded mode if either is unavailable.

## License

See [LICENSE](LICENSE) for details.
