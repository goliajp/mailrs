---
phase: 3
slug: webhook-subscriptions
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) |
| **Config file** | Cargo.toml workspace test config |
| **Quick run command** | `cargo test -p mailrs-server -- webhook` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p mailrs-server -- webhook`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | HOOK-01 | unit | `cargo test -p mailrs-server -- webhook::store::tests` | ❌ W0 | ⬜ pending |
| 03-01-02 | 01 | 1 | HOOK-06 | unit | `cargo test -p mailrs-server -- webhook::signer::tests` | ❌ W0 | ⬜ pending |
| 03-01-03 | 01 | 1 | HOOK-02, HOOK-03 | unit | `cargo test -p mailrs-server -- webhook::listener::tests` | ❌ W0 | ⬜ pending |
| 03-01-04 | 01 | 1 | HOOK-04, HOOK-05 | unit | `cargo test -p mailrs-server -- webhook::worker::tests` | ❌ W0 | ⬜ pending |
| 03-02-01 | 02 | 1 | HOOK-01 | integration | `cargo test -p mailrs-server -- webhook::api::tests` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/server/src/webhook/mod.rs` — module definition
- [ ] `crates/server/src/webhook/store.rs` — subscription + outbox CRUD with tests
- [ ] `crates/server/src/webhook/signer.rs` — HMAC signing with tests
- [ ] `crates/server/src/webhook/listener.rs` — event matching logic with tests
- [ ] `crates/server/src/webhook/worker.rs` — delivery + retry with tests
- [ ] `crates/server/src/web/webhook.rs` — API route handlers
- [ ] Schema migration: `webhook_subscriptions` + `webhook_outbox` tables

*All files are new — Wave 0 creates the module structure.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Webhook receives real HTTP callback | HOOK-05 | Requires running HTTP server | Start test server, create subscription, send email, verify callback received |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
