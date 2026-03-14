# mailrs

**mailrs** is a Rust mail server with SMTP inbound/outbound, IMAP, POP3, JMAP, ManageSieve, CalDAV/CardDAV, and a React web UI. It uses PostgreSQL (with pgvector), Valkey/Redis, and Maildir storage.

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

### Release & deployment

```bash
./scripts/release.sh          # test + bump patch + commit + tag + push + deploy
./scripts/release.sh minor    # bump minor version
./scripts/release.sh major    # bump major version
./scripts/release.sh 1.0.0    # bump to explicit version
./scripts/bump.sh <version>   # bump version only (no test/commit/deploy)
./scripts/deploy.sh           # deploy only (no version bump)
./scripts/dist.sh [target]    # build + package (default: aarch64-unknown-linux-gnu)
```

Deploy uses `cargo zigbuild` for cross-compilation. Expects `SSH_KEY` and `SSH_HOST` env vars.

### Release workflow (IMPORTANT)

**After ANY code change is complete (bug fix, feature, refactor), run `./scripts/release.sh` to release.** This is the standard workflow — do not skip it. The script:

1. Runs `cargo test --workspace` (all Rust tests)
2. Runs `bun run test` (all frontend tests)
3. Checks working tree is clean
4. Bumps version via `scripts/bump.sh`
5. Commits the version bump + tags `vX.Y.Z`
6. Pushes to origin (code + tags)
7. Cross-compiles and deploys to production

Use `patch` (default) for fixes and small changes, `minor` for features, `major` for breaking changes.

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
- **web** — REST API + WebSocket endpoints (Axum routes, auth middleware, admin APIs)
- **config** — All configuration via `MAILRS_*` environment variables
- **domain_store** — Domain/account/alias resolution from PostgreSQL (with Valkey + in-process DashMap cache)
- **event_bus** — Tokio broadcast channel (`SmtpEvent` enum) connecting SMTP/IMAP/web in real-time. SMTP sessions emit events, WebSocket handlers and IMAP IDLE forward them to clients
- **users** — Auth via PG `accounts` table (primary) with Argon2 password hashing, LDAP as fallback
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

PostgreSQL with pgvector extension. Schema in `scripts/init-schema.sql`. Key tables: `domains`, `accounts`, `aliases`, `mailboxes`, `messages`, `outbound_queue`, `greylist_triplets`, `dmarc_results`, `sieve_scripts`, `groups`, `group_permissions`, `account_groups`, `apps`, `api_keys`, `email_groups`, `email_group_members`, `signatures`, `encryption_keys`, `totp_secrets`, `audit_log`, `calendars`, `calendar_events`, `address_books`, `contacts`, `password_reset_tokens`.

sqlx is used with **runtime queries** (`sqlx::query` / `sqlx::query_as`), not compile-time checked macros — no `DATABASE_URL` needed at build time.

### Docker

Multi-stage Dockerfile: Rust builder → Bun web builder → Debian slim runtime. Compose provides postgres, valkey, and mailrs services.

### MCP

52 tools exposed at `/mcp` endpoint (Streamable HTTP, MCP protocol 2025-03-26). Categories: Email, Threads, Accounts, Permissions, Domains, Aliases, Email Groups, Apps, Webhooks, Queue, Signatures, Encryption Keys, Audit, Scheduled Send.

## Key Conventions

- Workspace dependencies are declared in root `Cargo.toml` and referenced with `workspace = true` in crate manifests
- Internal crate names are prefixed `mailrs-` (e.g., `mailrs-smtp-proto`) but directory names are not (e.g., `crates/smtp-proto`)
- Environment variables are the sole configuration mechanism — no config files except `users.toml` for credentials
- The inbound pipeline is ordered: each stage can reject early, reducing unnecessary processing
- Protocol crates (smtp-proto, imap-proto) are pure parsing/formatting with no I/O — session handlers in the server crate own the async I/O
- PG and Valkey are optional — the server starts in degraded mode if either is unavailable
- Auth primarily uses PG `accounts` table; `users.toml` is legacy fallback
