---
phase: 04-mcp-server
verified: 2026-03-10T09:30:00Z
status: gaps_found
score: 9/10 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 8/10
  gaps_closed:
    - "MCP auth middleware 验证 Bearer token 并将身份传递给 tool 方法"
  gaps_remaining:
    - "ROADMAP.md 状态未更新"
  regressions: []
gaps:
  - truth: "ROADMAP.md 状态反映 Phase 4 已完成"
    status: failed
    reason: "ROADMAP.md Progress 表仍显示 Phase 4 为 '0/2 | Not started'，Plans 复选框也未勾选"
    artifacts:
      - path: ".planning/ROADMAP.md"
        issue: "Line 93: '4. MCP Server | 0/2 | Not started | -'，应为 '2/2 | Complete | 2026-03-10'"
      - path: ".planning/ROADMAP.md"
        issue: "Line 80-81: Plans 复选框 '- [ ]' 未勾选为 '- [x]'"
      - path: ".planning/ROADMAP.md"
        issue: "Line 18: Phase 4 摘要行应标记 [x] 和 completed 日期"
    missing:
      - "更新 ROADMAP.md Phase 4 状态为 2/2 Complete，勾选两个 Plan 复选框，标记完成日期"
---

# Phase 4: MCP Server Verification Report

**Phase Goal:** Claude Code 等 AI agent 可通过 MCP 协议直接收发邮件
**Verified:** 2026-03-10T09:30:00Z
**Status:** gaps_found
**Re-verification:** Yes -- after gap closure (commit bc46d76)

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | MailMcpService 可创建并实现 ServerHandler trait | VERIFIED | `mcp/mod.rs` line 36-42: struct 定义, line 399-408: `#[tool_handler] impl ServerHandler` |
| 2 | send_email tool 接受参数并调用现有 send 逻辑 | VERIFIED | mod.rs line 54-133: 完整实现, 调用 verify_sender + build_rfc5322_message + deliver_message |
| 3 | read_email tool 通过 uid 返回邮件内容 | VERIFIED | mod.rs line 136-187: 遍历 mailbox 查找 uid, 解析 raw message 返回 JSON |
| 4 | search_emails tool 通过关键词搜索返回摘要列表 | VERIFIED | mod.rs line 189-226: 调用 mailbox_store.search_conversations, limit 上限 20 |
| 5 | reply_email tool 自动解析 thread 并设置 in_reply_to | VERIFIED | mod.rs line 228-344: resolve_thread_reply + auto "Re:" prefix + reply to last sender |
| 6 | list_conversations tool 返回会话列表摘要 | VERIFIED | mod.rs line 347-396: 调用 mailbox_store.list_conversations |
| 7 | MCP auth middleware 验证 Bearer token 并将身份传递给 tool 方法 | VERIFIED | auth.rs line 142: `MCP_AUTH_USER.scope(auth_user, next.run(request)).await` 设置 task-local; mod.rs line 21-24: `tokio::task_local!` 声明; mod.rs line 423-424: factory 通过 `MCP_AUTH_USER.try_with(\|u\| u.clone())` 读取认证用户 |
| 8 | GET/POST /mcp 端点响应 MCP protocol 初始化请求 | VERIFIED | web/mod.rs line 516: `setup_mcp(state.clone())`, line 519: auth middleware layer |
| 9 | 未认证请求 /mcp 返回 401 | VERIFIED | auth.rs line 25-26: 缺少 Bearer token 返回 401, line 29-31: 非 mlrs_ prefix 返回 401 |
| 10 | ROADMAP.md 状态反映 Phase 4 已完成 | FAILED | ROADMAP.md line 93: "0/2 | Not started", line 80-81: Plans 未勾选 |

**Score:** 9/10 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/server/src/mcp/mod.rs` | MailMcpService + ServerHandler + tools + setup_mcp() | VERIFIED | 439 lines, 5 tool 方法 + ServerHandler + setup_mcp + task-local AuthUser 传播 |
| `crates/server/src/mcp/tools.rs` | 5 parameter structs with JsonSchema + tests | VERIFIED | 127 lines, 5 structs + 8 unit tests |
| `crates/server/src/mcp/auth.rs` | mcp_auth_middleware | VERIFIED | 143 lines, Bearer token 验证 + task-local scope 设置 |
| `crates/server/src/web/mod.rs` | MCP router mounted | VERIFIED | line 516-520: MCP router + auth middleware layer |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| mcp/mod.rs (tools) | web/mail.rs | verify_sender, build_rfc5322_message, deliver_message | WIRED | mod.rs 直接调用 crate::web::mail:: 函数 (line 77, 93, 107, 245, 303, 317) |
| mcp/mod.rs (tools) | web/mod.rs (WebState) | self.web_state.* | WIRED | mailbox_store, hostname, maildir_root 均通过 self.web_state 访问 |
| mcp/auth.rs | api_key_store.rs | cache_get/get_api_key_by_prefix/sha256_hex | WIRED | auth.rs line 42, 55, 102: 完整调用链 |
| web/mod.rs | mcp/mod.rs | setup_mcp() | WIRED | web/mod.rs line 516: `crate::mcp::setup_mcp(state.clone())` |
| web/mod.rs | mcp/auth.rs | mcp_auth_middleware layer | WIRED | web/mod.rs line 519: `crate::mcp::auth::mcp_auth_middleware` |
| mcp/auth.rs | mcp/mod.rs (factory) | task-local MCP_AUTH_USER | WIRED | auth.rs line 142: scope 设置; mod.rs line 423: try_with 读取. middleware 和 factory 在同一 tokio task 中执行 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| MCP-01 | 04-01 | MCP server embedded using Rust rmcp | SATISFIED | rmcp 1.1 in Cargo.toml, MailMcpService implements ServerHandler |
| MCP-02 | 04-02 | Streamable HTTP transport at `/mcp` | SATISFIED | StreamableHttpService + nest_service("/mcp") + merged in router |
| MCP-03 | 04-01 | send_email tool via MCP | SATISFIED | `#[tool]` send_email 完整实现, 调用 deliver_message |
| MCP-04 | 04-01 | read_email tool via MCP | SATISFIED | `#[tool]` read_email 完整实现, 解析 maildir 消息 |
| MCP-05 | 04-01 | search_emails tool via MCP | SATISFIED | `#[tool]` search_emails 完整实现, 调用 search_conversations |
| MCP-06 | 04-01 | reply_email tool via MCP | SATISFIED | `#[tool]` reply_email 完整实现, resolve_thread_reply + auto Re: |
| MCP-07 | 04-01 | list_conversations tool via MCP | SATISFIED | `#[tool]` list_conversations 完整实现 |

No orphaned requirements found. All 7 MCP requirements are SATISFIED.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| mcp/mod.rs | 425-430 | Fallback 空白 AuthUser when task-local missing | INFO | 防御性编码, 仅在 middleware 未设置 task-local 时触发 (正常路径不会到达, middleware 会先返回 401) |

### Human Verification Required

### 1. Multi-User Auth Isolation

**Test:** 用两个不同用户的 API key 配置 Claude Code MCP server, 分别调用 list_conversations
**Expected:** 每个用户只看到自己的邮件
**Why human:** 需要两个 API key 实际测试, 确认 task-local 在并发请求下正确隔离

### Gaps Summary

**Auth identity 传播 (已修复):** commit bc46d76 引入 `tokio::task_local!` 机制, `mcp_auth_middleware` 通过 `MCP_AUTH_USER.scope(auth_user, next.run(request)).await` 将认证用户设置到 task-local, `setup_mcp()` 的 factory closure 通过 `MCP_AUTH_USER.try_with(|u| u.clone())` 读取。由于 axum middleware 和嵌套 service handler 在同一个 tokio task 中执行, task-local 在 factory closure 中始终可用。此 gap 已关闭。

**ROADMAP.md 状态 (未修复):** ROADMAP.md Progress 表仍显示 Phase 4 为 "0/2 | Not started", Plans 复选框未勾选。代码功能已完整, 这是纯文档维护问题。

---

_Verified: 2026-03-10T09:30:00Z_
_Verifier: Claude (gsd-verifier)_
