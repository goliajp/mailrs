# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**mailrs** is a Rust mail server with SMTP inbound/outbound, IMAP, and a React web UI. It uses PostgreSQL (with pgvector), Valkey/Redis, and Maildir storage.

## Build & Development Commands

### Prerequisites

Local dev requires PostgreSQL and Valkey running (use `docker compose up postgres valkey -d` to start them).

### Rust (workspace root)

```bash
cargo build                              # build all crates
cargo build -p mailrs-server             # build only the server
cargo test                               # test all crates
cargo test -p mailrs-smtp-proto          # test a single crate
cargo test -p mailrs-storage-maildir     # test a single crate (has integration tests)
cargo run --bin mailrs-server            # run the server (needs env vars, see scripts/dev.sh)
```

### Web frontend (web/)

Package manager is **Bun** (not npm/yarn).

```bash
cd web
bun install                              # install dependencies
bun run dev                              # vite dev server on :5173
bun run build                            # production build (tsc + vite)
bun run test                             # vitest
bun run lint                             # eslint
```

### Full local dev

```bash
./scripts/dev.sh    # starts cargo run + vite dev server with local env vars
```

This runs SMTP on 2525, submission on 2587, IMAP on 1143, web API on 3200, and Vite on 5173.

## Architecture

### Crate dependency graph

```
server (mailrs-server binary)
├── smtp-proto         — SMTP command parsing & session state machine
├── smtp-client        — Outbound SMTP connections, MX resolution, TLS
├── imap-proto         — IMAP command/response parsing & sequence sets
├── mailbox            — Mailbox metadata, message threading (sqlx + maildir)
│   └── storage-maildir — Maildir file format: flags, entries, message IDs
└── outbound-queue     — Delivery queue, retry logic, DKIM signing, DSN, MTA-STS
```

### Server crate modules (crates/server/src/)

The server binary wires everything together via Axum (web), Tokio (async), and direct TCP listeners:

- **smtp_session** / **imap_session** — Protocol handlers that drive the proto crates
- **inbound/pipeline** — Multi-stage email acceptance: rate limiting → PTR check → DNSBL → greylisting → SPF/DKIM/DMARC → content scan → sieve filtering → delivery
- **web/** — REST API + WebSocket endpoints (Axum routes, auth middleware, admin APIs)
- **config** — All configuration via `MAILRS_*` environment variables
- **domain_store** — Domain/account/alias resolution from PostgreSQL
- **users** — File-based user auth (users.toml) with Argon2 password hashing
- **acme** — Let's Encrypt certificate automation
- **tls** — TLS/STARTTLS setup with hot-reloadable certs (arc-swap)

### Web frontend (web/src/)

React 19 + TypeScript + Vite + Tailwind CSS 4 + Jotai state management.

- **pages/** — Route-level components (login, chat/conversation view, admin panels, protocol browser)
- **components/** — Conversation list, thread view, message bubbles, compose/reply forms
- **hooks/** — `use-mail-events.ts` and `use-smtp-events.ts` for WebSocket real-time updates
- **lib/api.ts** — HTTP client with bearer token auth
- **store/** — Jotai atoms for auth, mail, chat, and admin state

### Database

PostgreSQL with pgvector extension. Schema in `scripts/init-schema.sql`. Key tables: `domains`, `accounts`, `aliases`, `mailboxes`, `messages`, `outbound_queue`, `greylist_triplets`, `dmarc_results`, `sieve_scripts`.

sqlx is used with compile-time query checking macros.

### Docker

Multi-stage Dockerfile: Rust builder → Bun web builder → Debian slim runtime. Compose provides postgres, valkey, and mailrs services.

## Key Conventions

- Workspace dependencies are declared in root `Cargo.toml` and referenced with `workspace = true` in crate manifests
- Internal crate names are prefixed `mailrs-` (e.g., `mailrs-smtp-proto`) but directory names are not (e.g., `crates/smtp-proto`)
- Environment variables are the sole configuration mechanism — no config files except `users.toml` for credentials
- The inbound pipeline is ordered: each stage can reject early, reducing unnecessary processing
- Protocol crates (smtp-proto, imap-proto) are pure parsing/formatting with no I/O — session handlers in the server crate own the async I/O
