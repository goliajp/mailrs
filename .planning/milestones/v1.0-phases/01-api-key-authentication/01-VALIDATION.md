---
phase: 1
slug: api-key-authentication
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-03-10
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml |
| **Quick run command** | `cargo test -p mailrs-server` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p mailrs-server`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | AKEY-06 | unit | `cargo test -p mailrs-server` | ✅ (existing tests must still pass) | ⬜ pending |
| 01-01-02 | 01 | 1 | AKEY-02, AKEY-03, AKEY-07 | unit | `cargo test -p mailrs-server api_key` | ❌ W0 | ⬜ pending |
| 01-02-01 | 02 | 2 | AKEY-04 | integration | `cargo test -p mailrs-server api_key::tests::bearer_auth_works` | ❌ W0 | ⬜ pending |
| 01-02-02 | 02 | 2 | AKEY-01, AKEY-05 | integration | `cargo test -p mailrs-server api_key::tests::revoke_immediate_effect` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/server/src/api_key_store.rs` — API key DB/cache operations + unit tests
- [ ] `crates/server/src/web/api_key.rs` — CRUD handlers + integration tests
- [ ] `scripts/migrate-010-api-keys.sql` — DB migration
- [ ] Tests requiring PG can use `#[sqlx::test]` or mock the store trait

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| API key shown only once on creation | AKEY-02 | Response-level check, hard to assert in unit test | Create key via API, verify response contains full key, subsequent GET /keys does NOT return full key |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-03-10
