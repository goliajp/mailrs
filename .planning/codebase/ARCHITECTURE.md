# Architecture

**Analysis Date:** 2026-03-09

## Pattern Overview

**Overall:** Monolithic multi-protocol server with Rust workspace crates and a React SPA frontend

**Key Characteristics:**
- Single binary (`mailrs-server`) runs SMTP, IMAP, HTTP/WebSocket, and background workers concurrently via Tokio
- Protocol parsing crates are pure (no I/O); the server crate owns all async I/O and session handling
- Builder-pattern initialization: `main.rs` wires all subsystems together, passing `Arc<T>` shared state
- Graceful degradation: PG and Valkey are optional; the server starts in reduced-functionality mode if either is unavailable
- Event-driven real-time updates via `EventBus` (Tokio broadcast channel) connecting SMTP/IMAP/WebSocket

## Layers

**Protocol Parsing (Pure):**
- Purpose: Parse and format protocol commands/responses with no I/O
- Location: `crates/smtp-proto/src/`, `crates/imap-proto/src/`
- Contains: Command enums, parsers, response formatters, session state machines
- Depends on: Nothing (standalone)
- Used by: Server crate's session handlers

**Storage:**
- Purpose: Maildir file format operations and PG-backed mailbox metadata
- Location: `crates/storage-maildir/src/`, `crates/mailbox/src/`
- Contains: Maildir read/write (`storage-maildir`), mailbox CRUD + threading + conversation queries (`mailbox`)
- Depends on: sqlx (mailbox), filesystem (storage-maildir)
- Used by: Server crate's SMTP/IMAP sessions and web API

**Outbound Delivery:**
- Purpose: SMTP client connections, MX resolution, delivery queue with retry/bounce/DKIM
- Location: `crates/smtp-client/src/`, `crates/outbound-queue/src/`
- Contains: `SmtpConnection`, MX resolution (`smtp-client`); `DeliveryWorker`, queue management, DSN, MTA-STS (`outbound-queue`)
- Depends on: smtp-client, sqlx, hickory-resolver
- Used by: Server crate spawns `DeliveryWorker`

**Server Core:**
- Purpose: Wire all subsystems, run TCP listeners, handle sessions, serve web API
- Location: `crates/server/src/`
- Contains: `main.rs` (bootstrap), `smtp_session.rs`, `imap_session.rs`, `web/` (Axum routes), `inbound/pipeline.rs`, `config.rs`, `event_bus.rs`, `domain_store.rs`
- Depends on: All other crates, Axum, Tokio, sqlx, redis
- Used by: Binary entry point

**Web Frontend:**
- Purpose: React SPA for email reading, composing, admin
- Location: `web/src/`
- Contains: Pages, components, Jotai atoms, API client, hooks
- Depends on: REST API + WebSocket from server
- Used by: End users via browser

## Data Flow

**Inbound Email (SMTP -> Maildir + PG):**

1. TCP connection accepted on port 25/587 -> `smtp_session::handle_plain_connection()` in `crates/server/src/smtp_session.rs`
2. `SmtpCodec` frames lines; `mailrs_smtp_proto::Session` state machine processes commands
3. After DATA phase, inbound pipeline runs sequentially in `crates/server/src/inbound/pipeline.rs`:
   - Rate limiting -> PTR check -> DNSBL -> Greylisting -> SPF/DKIM/DMARC -> ClamAV scan -> Content rules -> AI spam check -> Sieve filtering
4. If accepted: message written to Maildir via `mailrs_storage_maildir`, metadata inserted into PG via `MailboxStore`
5. `EventBus` emits `SmtpEvent::NewMessage` -> WebSocket subscribers and IMAP IDLE sessions notified

**Outbound Email (Web API -> Queue -> SMTP Client):**

1. `POST /api/mail/send` handled by `crates/server/src/web/mail.rs`
2. Message composed, enqueued to `outbound_queue` PG table
3. `DeliveryWorker` (background task) polls queue, resolves MX, connects via `SmtpConnection`
4. DKIM signing applied if configured; retry with exponential backoff; DSN bounce generation on permanent failure
5. Delivery events bridged to `EventBus` for WebSocket notification

**Web UI Real-time Flow:**

1. Frontend connects WebSocket to `/api/events` via `web/src/hooks/use-mail-events.ts`
2. Server subscribes to `EventBus` and forwards `SmtpEvent::NewMessage` events as JSON
3. Frontend refreshes conversation list on new message; plays notification sound
4. Fallback: polling every 15 seconds if WebSocket disconnects

**State Management (Frontend):**
- Jotai atoms in `web/src/store/` (auth, chat, admin, settings, theme)
- No server-side session store beyond token -> `DashMap<String, SessionInfo>` in `WebState`
- API client in `web/src/lib/api.ts` uses Bearer token from localStorage

## Key Abstractions

**ConnectionContext:**
- Purpose: Shared immutable context for all SMTP connections
- Definition: `crates/server/src/smtp_session.rs` (struct `ConnectionContext`)
- Pattern: `Arc<ConnectionContext>` passed to every spawned connection handler
- Contains: hostname, maildir root, TLS state, user store, event bus, rate limiter, domain store, greylist config, auth guard, etc.

**WebState:**
- Purpose: Shared state for all Axum HTTP/WebSocket handlers
- Definition: `crates/server/src/web/mod.rs` (struct `WebState`)
- Pattern: Builder with `with_*()` methods, then wrapped in `Arc<WebState>` as Axum state
- Contains: event bus, PG pool, Valkey connection, mailbox store, domain store, sessions, rate limiter, health state

**EventBus:**
- Purpose: Decouple SMTP/IMAP/Web subsystems for real-time event propagation
- Definition: `crates/server/src/event_bus.rs`
- Pattern: Tokio broadcast channel wrapping `SmtpEvent` enum (14 variants)
- Usage: SMTP sessions emit events; WebSocket handler and IMAP IDLE subscribe

**Session (smtp-proto):**
- Purpose: Pure SMTP protocol state machine
- Definition: `crates/smtp-proto/src/session/`
- Pattern: `Session` struct with `State` enum; `process_line()` returns `Event` variants; no I/O
- States: Connected -> Greeted -> MailFrom -> RcptTo -> Data -> Completed

**MailboxStore:**
- Purpose: PG-backed mailbox metadata operations (CRUD, threading, conversations, search)
- Definition: `crates/mailbox/src/store.rs`
- Pattern: Wraps `PgPool`; runtime SQL queries (not compile-time checked)
- Key methods: `create_mailbox`, `append_message`, `get_conversations`, `search_conversations`, `semantic_search`

**DomainStore:**
- Purpose: Domain/account/alias resolution with 3-tier caching
- Definition: `crates/server/src/domain_store.rs`
- Pattern: L1 = in-process DashMap cache, L2 = Valkey, L3 = PostgreSQL
- Provides: `resolve_recipient()` -> `ResolvedRecipient::Account | Forward | Reject`

**Inbound Pipeline:**
- Purpose: Multi-stage email acceptance decision
- Definition: `crates/server/src/inbound/pipeline.rs`
- Pattern: Pure function `make_delivery_decision(PipelineInput) -> DeliveryDecision`; I/O stages run before calling it
- Stages feed `PipelineInput` with greylisting result, auth results, virus scan, content score, AI score

## Entry Points

**Server Binary:**
- Location: `crates/server/src/main.rs`
- Triggers: `cargo run --bin mailrs-server` or production binary
- Responsibilities: Parse env config, connect PG/Valkey, spawn TCP listeners (SMTP/submission/SMTPS/IMAP/IMAPS), spawn Axum web server, spawn background workers (delivery, DMARC reports, AI analyzer, content extraction, health checker, session cleanup)

**Web Frontend:**
- Location: `web/src/main.tsx` -> `web/src/app.tsx`
- Triggers: Browser loads SPA (served by Axum's static file fallback)
- Responsibilities: React Router routes to pages; `RequireAuth` guard redirects unauthenticated users

**REST API:**
- Location: `crates/server/src/web/mod.rs` (`router()` function)
- Triggers: HTTP requests to `/api/*`
- Routes organized by: auth (`/api/auth/*`), mail (`/api/mail/*`), conversations (`/api/conversations/*`), admin (`/api/admin/*`), queue (`/api/queue/*`), events WebSocket (`/api/events`)

## Error Handling

**Strategy:** Early rejection at protocol level; graceful degradation for infrastructure

**Patterns:**
- SMTP: Protocol-level error codes (4xx temporary, 5xx permanent) returned via `Response` types
- Inbound pipeline: `DeliveryDecision::Reject` with SMTP code + message, or `Greylist` for temporary deferral
- Web API: Axum extractors return `StatusCode` + JSON error body; 401 triggers frontend logout redirect
- Infrastructure: PG/Valkey failures logged with `eprintln!`, server continues in degraded mode
- Outbound: Exponential backoff retries; DSN bounce after max attempts

## Cross-Cutting Concerns

**Logging:** `eprintln!` for server startup/warnings; `tracing` crate available but primarily stderr-based
**Validation:** Input bounds enforced in web module (MAX_LIMIT, MAX_OFFSET, MAX_QUERY_LEN, MAX_BATCH_SIZE, MAX_RECIPIENTS, MAX_MULTIPART_BODY, MAX_ADMIN_FIELD_LEN, MAX_SIEVE_SCRIPT_LEN, MAX_EMAIL_BODY_LEN constants in `crates/server/src/web/mod.rs`)
**Authentication:** Dual-path: `users.toml` (file-based Argon2) checked first, then PG `accounts` table; Bearer token sessions stored in `DashMap` with 7-day TTL
**Rate Limiting:** SMTP: token bucket per IP (`crates/server/src/inbound/rate_limit.rs`); Web: per-IP middleware with stricter limits on auth endpoints (`crates/server/src/web/rate_limit.rs`)
**Security Headers:** CSP, X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy applied via Axum middleware in `crates/server/src/web/mod.rs`
**Brute Force Protection:** `AuthGuard` (`crates/server/src/inbound/auth_guard.rs`) tracks failed login attempts per account and per IP with exponential backoff lockout

---

*Architecture analysis: 2026-03-09*
