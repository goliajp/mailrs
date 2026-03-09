---
phase: 4
slug: mcp-server
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) |
| **Config file** | Cargo.toml workspace test config |
| **Quick run command** | `cargo test -p mailrs-server -- mcp` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p mailrs-server -- mcp`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | MCP-01 | unit | `cargo test -p mailrs-server -- mcp::tests` | ❌ W0 | ⬜ pending |
| 04-01-02 | 01 | 1 | MCP-03, MCP-04, MCP-05, MCP-06, MCP-07 | unit | `cargo test -p mailrs-server -- mcp::tools::tests` | ❌ W0 | ⬜ pending |
| 04-02-01 | 02 | 2 | MCP-02 | integration | `cargo test -p mailrs-server -- mcp` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/server/src/mcp/mod.rs` — MCP service struct + ServerHandler
- [ ] `crates/server/src/mcp/tools.rs` — tool implementations + unit tests
- [ ] `crates/server/src/mcp/auth.rs` — auth middleware for MCP route
- [ ] `Cargo.toml` dependency: `rmcp` + `schemars`

*All files are new — Wave 0 creates the module structure.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Claude Code can call MCP tools | MCP-02 | Requires live Claude Code + server | Configure MCP server in Claude Code, test send_email and search_emails |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
