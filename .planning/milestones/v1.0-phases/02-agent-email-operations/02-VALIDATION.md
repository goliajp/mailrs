---
phase: 2
slug: agent-email-operations
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-03-10
---

# Phase 2 — Validation Strategy

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
| 02-01-01 | 01 | 1 | MAIL-01, MAIL-02, MAIL-03, MAIL-06 | unit | `cargo build -p mailrs-server` | ✅ | ⬜ pending |
| 02-01-02 | 01 | 1 | MAIL-03, MAIL-06 | unit | `cargo test -p mailrs-server -- validate_from` | ❌ W0 | ⬜ pending |
| 02-02-01 | 02 | 1 | MAIL-02, MAIL-04, MAIL-05 | integration | `cargo test -p mailrs-server -- agent_mail` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Unit tests for superadmin from-address validation logic (MAIL-03)
- [ ] Unit tests for thread_id to Message-ID resolution (MAIL-06)
- [ ] Integration test for multipart attachment send via API key (MAIL-02)
- [ ] Integration tests for read/list/search via API key (MAIL-04, MAIL-05)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Email actually delivered to recipient | MAIL-01 | Requires live SMTP | Send via API, check recipient inbox |
| Attachment renders correctly in recipient client | MAIL-02 | Requires email client | Send with attachment, open in mail client |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-03-10
