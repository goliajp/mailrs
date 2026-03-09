---
phase: 03-webhook-subscriptions
verified: 2026-03-10T18:30:00Z
status: passed
score: 9/9 must-haves verified
re_verification: false
---

# Phase 3: Webhook Subscriptions Verification Report

**Phase Goal:** Agent 能订阅邮件事件并通过 webhook 接收实时通知
**Verified:** 2026-03-10T18:30:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Webhook subscription can be created, listed, and deleted in the database | VERIFIED | store.rs has create_subscription (L24-46), list_subscriptions (L49-62), delete_subscription (L65-78) with proper SQL |
| 2 | Outbox entry can be enqueued and dequeued with status transitions | VERIFIED | store.rs: enqueue_delivery (L118-134), dequeue_pending (L137-153), mark_inflight/delivered/failed (L156-208) |
| 3 | Webhook payload can be signed with HMAC-SHA256 and the signature verified | VERIFIED | signer.rs: sign_payload + verify_signature with timing-safe mac.verify_slice, 6 passing tests |
| 4 | EventBus NewMessage events are matched against active subscriptions and written to outbox | VERIFIED | listener.rs: run() subscribes to EventBus, matches SmtpEvent::NewMessage, calls find_matching_subscriptions + enqueue_delivery |
| 5 | Webhook worker polls outbox and delivers payloads via HTTP POST with HMAC signature | VERIFIED | worker.rs: WebhookWorker::run polls with 5s interval, deliver_one sends POST with 4 headers including X-Mailrs-Signature |
| 6 | Failed deliveries are retried with exponential backoff up to max_attempts | VERIFIED | store.rs mark_failed: attempt < max_attempts sets status='pending' with next_retry = now + retry_delay_secs; >= max_attempts sets status='failed' |
| 7 | Webhook payload contains only metadata (event, timestamp, user, thread_id, sender, subject, snippet) not full content | VERIFIED | mod.rs WebhookPayload/WebhookData structs contain only metadata fields, no body/content fields |
| 8 | Agent can create, list, and delete webhook subscriptions via REST API | VERIFIED | web/webhook.rs: create_webhook (POST), list_webhooks (GET), delete_webhook (DELETE); routes registered in web/mod.rs at /api/agent/webhooks |
| 9 | Webhook subscriptions can filter by sender email or thread ID | VERIFIED | listener.rs matches_subscription checks filter_sender and filter_thread_id; store.rs find_matching_subscriptions SQL uses (filter_sender IS NULL OR filter_sender = $3) pattern |

**Score:** 9/9 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `scripts/init-schema.sql` | webhook_subscriptions + webhook_outbox tables | VERIFIED | Both tables with proper columns, indexes, FK cascade |
| `crates/server/src/webhook/mod.rs` | Module definition + shared types | VERIFIED | 55 lines, exports 4 modules (store, signer, listener, worker), defines Subscription, OutboxEntry, WebhookPayload, WebhookData |
| `crates/server/src/webhook/store.rs` | PG CRUD + outbox queue | VERIFIED | 265 lines, 12 pub functions, 3 unit tests passing |
| `crates/server/src/webhook/signer.rs` | HMAC-SHA256 signing/verification | VERIFIED | 75 lines, sign_payload + verify_signature + format_signature_header, 6 unit tests passing |
| `crates/server/src/webhook/listener.rs` | EventBus subscriber + outbox enqueue | VERIFIED | 155 lines, matches_subscription pure function, 5 unit tests passing |
| `crates/server/src/webhook/worker.rs` | Background delivery worker | VERIFIED | 242 lines, poll-deliver-retry loop with semaphore(10) concurrency, 7 unit tests passing |
| `crates/server/src/web/webhook.rs` | Axum route handlers for webhook CRUD | VERIFIED | 239 lines, create/list/delete handlers with URL validation, 5 unit tests passing |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| listener.rs | event_bus.rs | EventBus::subscribe() + SmtpEvent::NewMessage | WIRED | L46: event_bus.subscribe(), L52: match SmtpEvent::NewMessage |
| listener.rs | store.rs | find_matching_subscriptions + enqueue_delivery | WIRED | L53: store::find_matching_subscriptions, L60: store::enqueue_delivery |
| worker.rs | signer.rs | sign_payload for X-Mailrs-Signature | WIRED | L116: signer::sign_payload, L117: signer::format_signature_header |
| worker.rs | store.rs | dequeue_pending + mark_* | WIRED | L51: store::dequeue_pending, L90: mark_inflight, L141: mark_delivered, L147/L153: mark_failed |
| web/webhook.rs | store.rs | create/list/delete subscription | WIRED | L105: store::create_subscription, L151: store::list_subscriptions, L190: store::delete_subscription |
| main.rs | listener.rs | tokio::spawn listener | WIRED | L666: webhook::listener::run(&eb, &pool_clone, rx) |
| main.rs | worker.rs | tokio::spawn worker | WIRED | L669: webhook::worker::WebhookWorker::new, L672: worker.run(rx) |
| web/mod.rs | web/webhook.rs | Route registration | WIRED | Routes at /api/agent/webhooks with post/get/delete handlers |
| Cargo.toml | hmac crate | hmac = "0.12" | WIRED | Line 48 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| HOOK-01 | 03-01, 03-02 | Agent can create webhook subscription (URL + event type) | SATISFIED | POST /api/agent/webhooks with url, event_type, returns signing_secret |
| HOOK-02 | 03-02 | Webhook can filter by contact email address | SATISFIED | filter_sender in CreateWebhookRequest, SQL WHERE filter_sender IS NULL OR filter_sender = $3 |
| HOOK-03 | 03-02 | Webhook can filter by thread ID | SATISFIED | filter_thread_id in CreateWebhookRequest, SQL WHERE filter_thread_id IS NULL OR filter_thread_id = $4 |
| HOOK-04 | 03-02 | Webhook payload contains only message ID + metadata (not full content) | SATISFIED | WebhookPayload has event, timestamp, data (user, thread_id, sender, subject, snippet) only |
| HOOK-05 | 03-02 | Failed webhook deliveries retry with exponential backoff | SATISFIED | mark_failed + retry_delay_secs: [60, 120, 300, 600, 1800, 3600, 7200, 21600]s, max 8 attempts |
| HOOK-06 | 03-01 | Webhook payload signed with HMAC-SHA256 | SATISFIED | signer::sign_payload with Hmac<Sha256>, X-Mailrs-Signature: sha256=... header |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns detected |

Compiler warnings for unused fields on Subscription and OutboxEntry are expected -- these fields are read by sqlx::FromRow but not directly accessed in Rust code beyond serialization. The `verify_signature` and `build_headers` unused warnings are for test-only / future-use functions.

### Human Verification Required

### 1. End-to-End Webhook Delivery

**Test:** Create a webhook subscription with a real URL (e.g., webhook.site), send an email, verify the webhook fires
**Expected:** POST arrives at webhook URL with correct Content-Type, X-Mailrs-Signature, X-Mailrs-Event headers and JSON metadata payload
**Why human:** Requires running server with PG, sending actual email through SMTP pipeline, verifying external HTTP delivery

### 2. Retry Behavior on Failure

**Test:** Create a webhook pointing to a non-existent URL, trigger a NewMessage event, check outbox entries after multiple poll cycles
**Expected:** Outbox entry shows increasing attempts count, next_retry values follow exponential backoff schedule
**Why human:** Requires running server and monitoring database state over time

### Gaps Summary

No gaps found. All 9 observable truths verified, all 7 artifacts exist and are substantive with proper wiring, all 6 requirements (HOOK-01 through HOOK-06) are satisfied. 26 unit tests pass covering store logic, signer, listener matching, worker headers, and URL validation.

---

_Verified: 2026-03-10T18:30:00Z_
_Verifier: Claude (gsd-verifier)_
