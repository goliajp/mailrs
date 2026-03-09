---
phase: 03-webhook-subscriptions
plan: 02
subsystem: api
tags: [webhook, eventbus, hmac, axum, reqwest, tokio]

requires:
  - phase: 03-webhook-subscriptions plan 01
    provides: store, signer, DB schema, shared types (Subscription, OutboxEntry, WebhookPayload)
provides:
  - EventBus listener that matches NewMessage events against webhook subscriptions
  - Background delivery worker with poll-deliver-retry loop and HMAC-signed payloads
  - REST API endpoints for webhook CRUD (POST/GET/DELETE /api/agent/webhooks)
  - Server wiring with graceful shutdown support
affects: [04-mcp-server]

tech-stack:
  added: []
  patterns: [semaphore-bounded concurrent delivery, outbox polling pattern, pure-function testable matching logic]

key-files:
  created:
    - crates/server/src/webhook/listener.rs
    - crates/server/src/webhook/worker.rs
    - crates/server/src/web/webhook.rs
  modified:
    - crates/server/src/webhook/mod.rs
    - crates/server/src/webhook/store.rs
    - crates/server/src/web/mod.rs
    - crates/server/src/main.rs

key-decisions:
  - "Extracted matches_subscription as pub(crate) pure function for unit testing without DB"
  - "Worker uses tokio::Semaphore(10) for bounded concurrency instead of JoinSet"
  - "URL validation allows http:// only for localhost/127.0.0.1 (dev convenience)"

patterns-established:
  - "Webhook matching: pure function extracted from async listener for testability"
  - "Header construction: build_headers pure function for testing delivery headers without HTTP"

requirements-completed: [HOOK-02, HOOK-03, HOOK-04, HOOK-05]

duration: 4min
completed: 2026-03-10
---

# Phase 3 Plan 2: Webhook Delivery Pipeline Summary

**EventBus listener matches NewMessage events against filtered subscriptions, background worker delivers HMAC-signed payloads with exponential backoff retry, REST API for webhook CRUD**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-09T17:57:09Z
- **Completed:** 2026-03-09T18:01:35Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- EventBus listener subscribes to NewMessage events and matches against active subscriptions with sender/thread_id filters
- Delivery worker polls outbox, delivers HTTP POST with HMAC-SHA256 signed payloads, handles success/failure with retry
- REST API for webhook CRUD: create (with signing_secret response), list (secret omitted), delete (soft-delete)
- All 31 webhook tests pass (26 existing + 5 new URL validation tests), zero regressions workspace-wide

## Task Commits

Each task was committed atomically:

1. **Task 1: EventBus listener + delivery worker** - `4a7c5b9` (feat)
2. **Task 2: API routes + server wiring** - `f7bfbf6` (feat)

## Files Created/Modified
- `crates/server/src/webhook/listener.rs` - EventBus subscriber with subscription matching and outbox enqueue
- `crates/server/src/webhook/worker.rs` - Background delivery worker with poll-deliver-retry loop
- `crates/server/src/web/webhook.rs` - Axum route handlers for webhook CRUD
- `crates/server/src/webhook/mod.rs` - Added listener and worker module declarations
- `crates/server/src/webhook/store.rs` - Added get_subscription for worker to load signing_secret
- `crates/server/src/web/mod.rs` - Added webhook module and routes
- `crates/server/src/main.rs` - Spawned webhook listener and worker tasks

## Decisions Made
- Extracted `matches_subscription` as a pure `pub(crate)` function for direct unit testing without DB
- Used `tokio::Semaphore(10)` for bounded concurrent delivery instead of JoinSet
- URL validation allows `http://` only for `localhost` and `127.0.0.1` to support local development
- Worker builds all 4 headers (Content-Type, X-Mailrs-Signature, X-Mailrs-Event, X-Mailrs-Delivery) via extracted `build_headers` function

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Webhook system complete end-to-end: EventBus -> listener -> outbox -> worker -> HTTP delivery
- Phase 4 (MCP server) can proceed independently
- All webhook tests pass, ready for release

---
*Phase: 03-webhook-subscriptions*
*Completed: 2026-03-10*
