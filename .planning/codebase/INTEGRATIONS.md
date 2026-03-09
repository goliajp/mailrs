# External Integrations

**Analysis Date:** 2026-03-09

## APIs & External Services

**AI - Spam Classification (Anthropic Claude):**
- Purpose: AI-powered spam scoring for inbound emails
- SDK/Client: `reqwest` 0.12 (direct HTTP calls)
- Implementation: `crates/server/src/ai_spam.rs`
- API: `https://api.anthropic.com/v1/messages`
- Model: `claude-haiku-4-5-20251001` (default, configurable via `MAILRS_AI_MODEL`)
- Auth: `MAILRS_AI_API_KEY` env var (passed as `x-api-key` header with `anthropic-version: 2023-06-01`)
- Disabled by default (`MAILRS_AI_ENABLED=false`)

**AI - Email Analysis (Google Gemini):**
- Purpose: Email categorization, summarization, entity extraction, and embedding generation
- SDK/Client: `reqwest` 0.12 (direct HTTP calls to Google generativelanguage API)
- Implementation: `crates/server/src/ai_email.rs`, `crates/server/src/ai_analyzer.rs`
- Embedding API: `https://generativelanguage.googleapis.com/v1beta/models/{model}:embedContent`
- Analysis API: `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
- Models: `gemini-embedding-001` (embeddings, 768-dim), `gemini-2.5-flash` (analysis)
- Auth: `MAILRS_GEMINI_API_KEY` env var (passed as `?key=` query parameter)
- Disabled by default (`MAILRS_AI_ANALYSIS_ENABLED=false`)

**DNS Services:**
- Purpose: MX resolution, PTR lookups, DNSBL queries, SPF/DKIM/DMARC verification
- SDK/Client: `hickory-resolver` 0.25, `mail-auth` 0.7
- Implementation: `crates/smtp-client/`, `crates/outbound-queue/`, `crates/server/src/inbound/`
- No auth required (public DNS)

**Let's Encrypt (ACME):**
- Purpose: Automatic TLS certificate provisioning and renewal
- SDK/Client: `instant-acme` 0.8
- Implementation: `crates/server/src/acme.rs`
- Auth: `MAILRS_ACME_EMAIL` (account registration)
- Config: `MAILRS_ACME_DOMAINS`, `MAILRS_ACME_DIR`, `MAILRS_ACME_STAGING`

**ClamAV (Optional):**
- Purpose: Virus/malware scanning of inbound email
- Connection: TCP socket to ClamAV daemon
- Auth: `MAILRS_CLAMAV_ADDR` env var (e.g., `127.0.0.1:3310`)
- Not required - disabled if addr not set

## Data Storage

**PostgreSQL (with pgvector):**
- Image: `pgvector/pgvector:pg18`
- Connection: `MAILRS_PG_URL` env var (e.g., `postgres://mailrs:mailrs@postgres:5432/mailrs`)
- Client: `sqlx` 0.8.6 (runtime queries, not compile-time checked)
- Schema: `scripts/init-schema.sql`
- Extensions: `vector` (pgvector for 768-dim embeddings)
- Tables: `domains`, `accounts`, `aliases`, `mailboxes`, `messages`, `outbound_queue`, `greylist_triplets`, `dmarc_results`, `sieve_scripts`, `contacts`, `sender_feedback`, `email_analysis`, `reactions`, `snoozed_conversations`
- Migrations: `scripts/migrate-*.sql` (applied manually during deploy)
- Optional: server starts in degraded mode if unavailable

**Valkey/Redis:**
- Image: `valkey/valkey:8`
- Connection: `MAILRS_VALKEY_URL` env var (e.g., `redis://valkey:6379`)
- Client: `redis` 0.27 crate with `tokio-comp` and `connection-manager`
- Purpose: Caching (domain/account lookups, auth state), rate limiting data
- Also used by: `crates/outbound-queue/` for delivery queue coordination
- Optional: server starts in degraded mode if unavailable

**Maildir (File Storage):**
- Path: `MAILRS_MAILDIR` env var (default `/tmp/mailrs` dev, `/data/maildir` production)
- Implementation: `crates/storage-maildir/` - custom Maildir format handler
- Purpose: Raw email message storage (RFC Maildir++ format with flags)
- Docker volume: `mailrs-data` mounted at `/data`

**Caching:**
- Valkey/Redis for distributed cache
- In-process `DashMap` for hot domain/account lookups (`crates/server/src/domain_store.rs`)
- Two-tier: DashMap (L1) → Valkey (L2) → PostgreSQL (L3)

## Authentication & Identity

**User Authentication:**
- Primary: PostgreSQL `accounts` table with `password_hash` (Argon2)
- Fallback: File-based auth via `users.toml` (`MAILRS_USERS_FILE`)
- Implementation: `crates/server/src/users.rs`
- Password hashing: `argon2` 0.5 crate
- Brute force protection: Configurable lockout (per-account and per-IP with exponential backoff)

**Web API Auth:**
- Bearer token authentication
- Token stored in `localStorage` as `mailrs_auth`
- Implementation: `crates/server/src/web/` (Axum middleware)

**Email Authentication (Protocol):**
- SPF verification: `mail-auth` 0.7
- DKIM signing/verification: `mail-auth` 0.7
- DMARC policy checking: `mail-auth` 0.7
- MTA-STS enforcement: `crates/outbound-queue/src/mta_sts.rs`

## Monitoring & Observability

**Error Tracking:**
- None (no Sentry/similar)

**Logs:**
- `tracing` 0.1 crate for structured logging
- Container logs via Docker (`docker compose logs`)

**Health Check:**
- HTTP endpoint: `GET /api/health`
- Docker HEALTHCHECK configured (30s interval, 5s timeout)

## CI/CD & Deployment

**Hosting:**
- Self-hosted on remote Linux server (aarch64) at `t02.golia.jp`
- Docker Compose orchestration (postgres + valkey + mailrs)

**CI Pipeline:**
- No CI service (no GitHub Actions, no Jenkins)
- Manual release via `scripts/release.sh` (test → bump → tag → push → deploy)

**Build Pipeline:**
- `cargo zigbuild --release --target aarch64-unknown-linux-gnu` for cross-compilation
- `bunx --bun vite build` for frontend
- Docker multi-stage build for container image (`Dockerfile`)

**Deployment:**
- SSH/SCP-based deployment (`scripts/deploy.sh`)
- Requires: `SSH_KEY` and `SSH_HOST` env vars
- Process: upload binary + web assets + configs → rebuild Docker → restart

## Environment Configuration

**Required env vars (production):**
- `MAILRS_HOSTNAME` - Server FQDN (e.g., `mx.golia.jp`)
- `MAILRS_LOCAL_DOMAINS` - Comma-separated list of served domains
- `MAILRS_PG_URL` - PostgreSQL connection string
- `MAILRS_VALKEY_URL` - Valkey/Redis connection string
- `MAILRS_MAILDIR` - Maildir storage path

**Optional env vars:**
- `MAILRS_TLS_CERT` / `MAILRS_TLS_KEY` - Manual TLS certificate paths
- `MAILRS_ACME_EMAIL` / `MAILRS_ACME_DOMAINS` - Auto TLS via Let's Encrypt
- `MAILRS_DKIM_SELECTOR` / `MAILRS_DKIM_DOMAIN` / `MAILRS_DKIM_PRIVATE_KEY` - DKIM signing
- `MAILRS_USERS_FILE` - Path to `users.toml` for file-based auth
- `MAILRS_AI_ENABLED` / `MAILRS_AI_API_KEY` / `MAILRS_AI_MODEL` - Anthropic spam classification
- `MAILRS_GEMINI_API_KEY` / `MAILRS_AI_ANALYSIS_ENABLED` - Gemini email analysis
- `MAILRS_CLAMAV_ADDR` - ClamAV daemon address
- `MAILRS_ANTISPAM_ENABLED` - Enable/disable full anti-spam pipeline
- `MAILRS_WEB_STATIC_DIR` - Path to built frontend assets

**Secrets location:**
- `.env.local` file locally (gitignored)
- `.env` file on remote server (deployed by `scripts/deploy.sh`)

## WebSocket / Real-time

**Incoming (server → client):**
- Endpoint: `GET /api/events?token=<jwt>` - WebSocket upgrade
- Events: `SmtpEvent` (SMTP protocol events), `NewMessage` (new email notification)
- Implementation: `crates/server/src/web/ws.rs`
- Internal bus: Tokio broadcast channel (`SmtpEvent` enum) in `crates/server/src/event_bus.rs`
- Client: `web/src/hooks/use-mail-events.ts` (auto-reconnect, 30s ping, 15s poll fallback)

**SMTP Monitoring WebSocket:**
- Endpoint: dedicated SMTP event stream
- Client: `web/src/hooks/use-smtp-events.ts`

## Webhooks & Callbacks

**Incoming:**
- None (no inbound webhooks)

**Outgoing:**
- DMARC aggregate reports stored in `dmarc_results` table (no outbound webhook)
- DSN (Delivery Status Notifications) sent via SMTP for bounced messages

---

*Integration audit: 2026-03-09*
