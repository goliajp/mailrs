---
phase: 03-webhook-subscriptions
plan: 01
subsystem: api
tags: [webhook, hmac, sha256, sqlx, postgres, outbox]

requires:
  - phase: 01-api-key-auth
    provides: API key auth extractor and account_address pattern
provides:
  - webhook_subscriptions and webhook_outbox DB tables
  - webhook::store module with CRUD and outbox queue operations
  - webhook::signer module with HMAC-SHA256 signing and verification
  - shared types (Subscription, OutboxEntry, WebhookPayload, WebhookData)
affects: [03-02 webhook listener/worker/api routes]

tech-stack:
  added: [hmac 0.12]
  patterns: [DB outbox queue, HMAC-SHA256 payload signing, exponential backoff retry]

key-files:
  created:
    - crates/server/src/webhook/mod.rs
    - crates/server/src/webhook/store.rs
    - crates/server/src/webhook/signer.rs
  modified:
    - scripts/init-schema.sql
    - crates/server/Cargo.toml
    - crates/server/src/main.rs

key-decisions:
  - "signing_secret stored as plaintext in DB (required for HMAC computation, cannot be hashed)"
  - "retry delays match outbound_queue pattern: 60s to 6h exponential backoff with 8 max attempts"

patterns-established:
  - "webhook::store pattern: async PgPool functions for subscription CRUD and outbox queue management"
  - "webhook::signer pattern: sign_payload/verify_signature with timing-safe HMAC comparison"

requirements-completed: [HOOK-01, HOOK-06]

duration: 3min
completed: 2026-03-10
---

# Phase 3 Plan 1: Webhook Data Layer Summary

**DB schema for webhook subscriptions/outbox, store module with CRUD + queue ops, and HMAC-SHA256 signer with timing-safe verification**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-09T17:50:37Z
- **Completed:** 2026-03-09T17:54:05Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- webhook_subscriptions and webhook_outbox tables with proper indexes appended to init-schema.sql
- Complete store module with subscription CRUD, outbox enqueue/dequeue, status transitions, and exponential backoff retry
- HMAC-SHA256 signer with timing-safe verification via hmac crate's verify_slice
- 9 unit tests covering retry delays, secret generation, payload serialization, signing, and verification

## Task Commits

Each task was committed atomically:

1. **Task 1: DB schema + webhook store module** - `4198547` (feat)
2. **Task 2: HMAC-SHA256 signer module** - `e6a3a02` (feat)

## Files Created/Modified
- `scripts/init-schema.sql` - Added webhook_subscriptions and webhook_outbox table definitions with indexes
- `crates/server/Cargo.toml` - Added hmac = "0.12" dependency
- `crates/server/src/main.rs` - Added mod webhook declaration
- `crates/server/src/webhook/mod.rs` - Module definition with shared types (Subscription, OutboxEntry, WebhookPayload, WebhookData)
- `crates/server/src/webhook/store.rs` - PG CRUD for subscriptions + outbox queue operations with unit tests
- `crates/server/src/webhook/signer.rs` - HMAC-SHA256 signing and verification with unit tests

## Decisions Made
- signing_secret stored as plaintext in DB (HMAC computation requires the original secret, unlike API keys which can be hashed)
- Retry delay schedule matches outbound_queue pattern: [60, 120, 300, 600, 1800, 3600, 7200, 21600] seconds

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Store and signer modules ready for Plan 02 (listener, worker, API routes)
- All types exported for use by listener (find_matching_subscriptions) and worker (dequeue_pending, mark_*)
- Signer ready for worker to sign payloads before HTTP delivery

---
*Phase: 03-webhook-subscriptions*
*Completed: 2026-03-10*
