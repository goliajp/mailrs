# Codebase Concerns

**Analysis Date:** 2026-03-09

## Tech Debt

**Oversized files exceeding 800-line guideline:**
- Issue: 12 files exceed 800 lines, with several over 1500. Large files reduce readability and increase merge conflict risk.
- Files:
  - `crates/server/src/imap_session.rs` (2480 lines) — IMAP state machine + handlers + tests all in one
  - `crates/mailbox/src/store.rs` (2414 lines) — all mailbox SQL queries in a single file
  - `crates/server/src/config.rs` (1905 lines) — config struct + parsing + defaults + validation + tests
  - `crates/imap-proto/src/command.rs` (1749 lines) — IMAP command parser
  - `crates/server/src/web/mail.rs` (1546 lines) — all mail API handlers
  - `crates/server/src/sieve.rs` (1445 lines) — sieve compiler + evaluator + tests
  - `crates/server/src/inbound/pipeline.rs` (1346 lines) — inbound pipeline + decision logic + tests
  - `crates/server/src/smtp_session.rs` (1274 lines) — SMTP session handler
  - `crates/server/src/web/conversations.rs` (1261 lines) — conversation API handlers, no tests
  - `crates/server/src/domain_check.rs` (1208 lines) — DNS validation
  - `crates/server/src/domain_store.rs` (1121 lines) — domain/account storage
  - `crates/server/src/web/mod.rs` (1108 lines) — web state + router + helpers + tests
- Impact: Hard to navigate, high cognitive load, slow code reviews
- Fix approach: Extract logical groups into sub-modules. For example, split `imap_session.rs` into `imap_session/state.rs`, `imap_session/commands.rs`, `imap_session/fetch.rs`. Split `store.rs` into `store/conversations.rs`, `store/messages.rs`, `store/mailboxes.rs`.

**Mixed logging — eprintln! vs tracing:**
- Issue: The server crate uses `eprintln!` for most logging (93 calls) alongside sporadic `tracing::info/warn/error` calls (10+ calls in `dmarc_report.rs`, `ai_analyzer.rs`, `conversations.rs`, `auth_guard.rs`). No structured logging middleware.
- Files: Nearly all files in `crates/server/src/`
- Impact: No structured log output, no log levels, no request correlation in production. Hard to filter/search logs.
- Fix approach: Migrate all `eprintln!` to `tracing` macros. Add `tracing-subscriber` initialization in `crates/server/src/main.rs`. Already have `tracing` as a dependency since some modules use it.

**Dynamic SQL construction in mailbox store:**
- Issue: `crates/mailbox/src/store.rs` builds SQL queries via `format!()` with dynamic WHERE clauses and parameter indices. While parameter values are bound (not interpolated), the query structure itself is built dynamically, making it fragile and hard to audit.
- Files: `crates/mailbox/src/store.rs` (lines 8-22, 625-655, 750-756, 1250-1270)
- Impact: Risk of parameter index mismatch bugs; hard to read and maintain
- Fix approach: Consider using a query builder crate (e.g., `sea-query`) or at minimum extract query construction into well-tested helper functions with the current `build_user_filter` pattern applied consistently.

**Frontend component size:**
- Issue: `web/src/components/conversation-list.tsx` (977 lines) and `web/src/components/thread-view.tsx` (816 lines) exceed reasonable component size.
- Files: `web/src/components/conversation-list.tsx`, `web/src/components/thread-view.tsx`
- Impact: Hard to test, reason about, and reuse sub-behaviors
- Fix approach: Extract sub-components (list item, filter bar, batch action bar) and custom hooks for data fetching/state management.

## Security Considerations

**CORS allows any origin:**
- Risk: `allow_origin(tower_http::cors::Any)` permits requests from any domain. Combined with bearer token auth, this could enable CSRF-like attacks if tokens are leaked.
- Files: `crates/server/src/web/mod.rs` (line 736)
- Current mitigation: Bearer token auth (not cookies), so traditional CSRF is not applicable. Rate limiting is in place.
- Recommendations: Restrict CORS to the actual frontend origin in production. Use environment variable `MAILRS_CORS_ORIGIN` to configure allowed origins.

**WebSocket endpoint allows unauthenticated connections:**
- Risk: The `/api/events` WebSocket endpoint accepts connections without a token (`token` query param is optional). Any client can connect and receive all `SmtpEvent` broadcasts including email metadata.
- Files: `crates/server/src/web/ws.rs` (lines 22-28)
- Current mitigation: Token is validated if provided, but connection proceeds without one.
- Recommendations: Make token mandatory. Reject WebSocket upgrade if no valid token is present.

**Auth token in URL query parameters:**
- Risk: Bearer tokens are passed via `?token=` query parameter for attachment downloads, inline images, and WebSocket connections. URL tokens can leak via server logs, browser history, Referer headers, and proxy logs.
- Files: `crates/server/src/web/auth.rs` (lines 42-51), `web/src/components/attachment-preview.tsx` (line 24), `web/src/hooks/use-mail-events.ts` (line 95)
- Current mitigation: None
- Recommendations: Use short-lived, single-use download tokens generated specifically for resource access, rather than reusing the session token.

**No admin role enforcement on admin API endpoints:**
- Risk: Admin routes (`/api/admin/domains`, `/api/admin/accounts`, etc.) only check `AuthUser` — any authenticated user can add/remove domains, accounts, and aliases. There is no admin role or privilege check.
- Files: `crates/server/src/web/admin.rs` (all handlers use `AuthUser(_user)` without role check), `crates/server/src/web/mod.rs` (lines 672-709)
- Current mitigation: `super_domains` permission limits cross-domain data access, but does not restrict administrative mutations.
- Recommendations: Add admin role check. Either use `super_domains` presence as admin indicator or add an explicit `is_admin` flag to accounts.

**Plaintext password support in users.toml:**
- Risk: `UserStore` supports plaintext passwords via the `password` field alongside Argon2 hashes (`password_hash`).
- Files: `crates/server/src/users.rs` (lines 16-22, 75-81)
- Current mitigation: `password_hash` takes precedence when both are present.
- Recommendations: Deprecate plaintext support. Auto-hash on load and warn loudly.

## Performance Bottlenecks

**Mutex-based MX cache in async context:**
- Problem: `MxCache` in `crates/smtp-client/src/mx.rs` uses `std::sync::Mutex` with `.unwrap()` on lock acquisition. In an async Tokio runtime, holding a std Mutex across `.await` points could block the runtime, and a poisoned mutex would panic the server.
- Files: `crates/smtp-client/src/mx.rs` (lines 107, 120, 129, 135, 140)
- Cause: `std::sync::Mutex` is not designed for async runtimes
- Improvement path: Replace with `tokio::sync::Mutex` or `dashmap::DashMap` (already used elsewhere in the codebase) for async-safe cache access. Replace `.unwrap()` with proper error handling.

**Session lookup by value in validate_domains:**
- Problem: `validate_domains()` iterates all sessions to find the one matching a user address, rather than looking up by token.
- Files: `crates/server/src/web/mod.rs` (lines 273-279)
- Cause: Sessions are keyed by token, not by user address
- Improvement path: Add a reverse index (user -> session) or pass session info through the request extensions from `AuthUser` extractor.

**No pagination limit enforcement on conversation queries:**
- Problem: While `clamp_limit` and `clamp_offset` exist, some query paths in `store.rs` could return large result sets if the client provides unexpected parameters.
- Files: `crates/mailbox/src/store.rs`, `crates/server/src/web/conversations.rs`
- Cause: Complex query construction makes it easy to miss limit enforcement
- Improvement path: Enforce hard limits at the SQL layer (always append LIMIT clause).

## Fragile Areas

**IMAP session state machine:**
- Files: `crates/server/src/imap_session.rs`
- Why fragile: 2480-line file with complex state transitions (NotAuthenticated -> Authenticated -> Selected). Handles all IMAP commands in a giant match. State is mutable and transitions are scattered throughout.
- Safe modification: Always add new commands in the appropriate state branch. Run the full IMAP test suite (`cargo test -p mailrs-server -- imap`). Test both authenticated and unauthenticated states.
- Test coverage: Has 77 inline tests plus integration tests in `crates/server/tests/e2e.rs`. Reasonable coverage but the monolithic structure makes edge cases hard to isolate.

**Dynamic SQL query construction in MailboxStore:**
- Files: `crates/mailbox/src/store.rs`
- Why fragile: Parameter index tracking (`param_idx`) is manual across complex conditional query building. Off-by-one in parameter indices causes silent wrong-data bugs, not compilation errors.
- Safe modification: Always verify `param_idx` tracking after adding conditions. Run the MailboxStore test suite. Manually test with multiple filter combinations.
- Test coverage: 33 tests, but testing all parameter index paths requires many combinations that may not be fully covered.

**Inbound pipeline ordering:**
- Files: `crates/server/src/inbound/pipeline.rs`, `crates/server/src/smtp_session.rs`
- Why fragile: The pipeline stages (rate limiting -> PTR check -> DNSBL -> greylisting -> SPF/DKIM/DMARC -> content scan -> sieve) have implicit ordering dependencies. Reordering stages could bypass security checks.
- Safe modification: The `make_delivery_decision()` function is pure and well-tested (64 tests). Changes to pipeline orchestration in `smtp_session.rs` are riskier — test with real SMTP traffic.
- Test coverage: Decision logic is well covered. Integration of stages in `smtp_session.rs` relies on e2e tests.

## Scaling Limits

**In-process session storage:**
- Current capacity: Sessions stored in `DashMap<String, SessionInfo>` in memory
- Limit: Cannot scale horizontally — each server instance has its own session store. Memory grows linearly with active sessions.
- Scaling path: Move sessions to Valkey/Redis (already available in the stack). The `valkey` connection is present in `WebState` but unused for session management.

**Single-process architecture:**
- Current capacity: All protocols (SMTP, IMAP, HTTP/WebSocket) run in a single Tokio runtime
- Limit: Cannot independently scale SMTP vs web vs IMAP. A spike in SMTP traffic affects web API responsiveness.
- Scaling path: Split into separate binaries per protocol, sharing PG/Valkey backends.

## Dependencies at Risk

**Plaintext password in UserEntry:**
- Risk: The `password` field in `UserEntry` struct encourages storing plaintext passwords.
- Impact: If `users.toml` is leaked, plaintext passwords are immediately compromised.
- Migration plan: Remove `password` field support. Provide a migration CLI to hash existing plaintext passwords.

## Test Coverage Gaps

**Web conversations module — 0 tests:**
- What's not tested: All 23 async handler functions in the conversations API (list, search, semantic search, batch operations, pin, archive, snooze, feedback, delete)
- Files: `crates/server/src/web/conversations.rs` (1261 lines, 0 `#[test]` annotations)
- Risk: Any refactoring of conversation queries or response formatting could silently break the most user-facing API
- Priority: High

**Web admin module — 0 tests:**
- What's not tested: Domain/account/alias CRUD, quota management, sieve script management, queue operations
- Files: `crates/server/src/web/admin.rs` (700 lines, 0 `#[test]` annotations)
- Risk: Admin operations could silently corrupt data
- Priority: High

**Web auth module — 0 tests:**
- What's not tested: Login flow, session creation, token validation, logout, `AuthUser` extractor
- Files: `crates/server/src/web/auth.rs` (210 lines, 0 `#[test]` annotations)
- Risk: Authentication bypass bugs would go undetected
- Priority: High

**ACME module — 0 tests:**
- What's not tested: Let's Encrypt certificate automation
- Files: `crates/server/src/acme.rs` (307 lines)
- Risk: Certificate renewal failures in production
- Priority: Medium

**AI analyzer module — 0 tests:**
- What's not tested: Gemini API integration, email analysis backfill logic
- Files: `crates/server/src/ai_analyzer.rs` (334 lines)
- Risk: Silent failures in AI analysis pipeline
- Priority: Medium

**Frontend test coverage:**
- What's not tested: Most page components (`pages/admin-accounts.tsx`, `pages/settings.tsx`, `pages/protocol.tsx`, `pages/playground.tsx`), some core components (`rich-editor.tsx`, `new-conversation.tsx`, `message-bubble.tsx` partially)
- Files: `web/src/pages/` (all page components), several `web/src/components/` files
- Risk: UI regressions on page-level components
- Priority: Medium

---

*Concerns audit: 2026-03-09*
