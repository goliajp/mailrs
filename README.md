# mailrs

An AI-native mail server written in Rust with a React web UI. Handles SMTP inbound/outbound, IMAP, and provides a modern conversational email interface.

## Architecture

mailrs is organized as a Cargo workspace with 7 crates:

```
server (mailrs-server binary)
├── smtp-proto         — SMTP command parsing & session state machine
├── smtp-client        — Outbound SMTP connections, MX resolution, TLS
├── imap-proto         — IMAP command/response parsing & sequence sets
├── mailbox            — Mailbox metadata, message threading (sqlx + maildir)
│   └── storage-maildir — Maildir file format: flags, entries, message IDs
└── outbound-queue     — Delivery queue, retry logic, DKIM signing, DSN, MTA-STS
```

Protocol crates (`smtp-proto`, `imap-proto`) are pure parsing with no I/O — the server crate owns all async networking via Tokio.

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
