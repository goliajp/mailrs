# Phase 2: Agent Email Operations - Research

**Researched:** 2026-03-10
**Domain:** REST API email operations for AI agents (Rust/Axum)
**Confidence:** HIGH

## Summary

Phase 2 的核心发现是：**大部分所需端点已经存在且可直接通过 API key 认证使用**。Phase 1 完成的统一 `AuthUser` extractor 意味着所有现有 mail/conversations 端点已自动支持 API key 认证。

需要做的改动集中在三个方面：(1) `send_message` 的 from 地址校验逻辑需要为 superadmin 放开限制 (MAIL-03)；(2) `send_message_multipart` 已存在但需验证其对 agent 场景的可用性 (MAIL-02)；(3) 需要一个专门的 reply 端点或确认现有 `in_reply_to` 机制对 agent 足够友好 (MAIL-06)。

**Primary recommendation:** 不要创建新的 agent 专用端点，而是调整现有端点的少量逻辑（superadmin from 校验），并确保 API 对无状态调用者（agent）返回清晰的响应结构。

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MAIL-01 | Agent can send email via API (to/cc/bcc, subject, text/html body) | `POST /api/mail/send` 已存在，接受 `SendMessageRequest` JSON，支持 to/cc/bcc/subject/body/html_body。API key auth 已生效。**几乎无需改动**，仅需让 from 字段在为空时自动填充 `AuthUser.address` |
| MAIL-02 | Agent can send email with attachments (multipart/form-data) | `POST /api/mail/send-multipart` 已存在，使用 axum `Multipart` extractor。已有 `AttachmentData` struct 和 25MB body limit。需验证当前实现完整性 |
| MAIL-03 | Superadmin key can specify arbitrary from address | `send_message` 当前硬拒绝 `from != user`。需修改：当 `AuthUser.super_domains` 非空时，验证 from 的 domain 在 super_domains 中即可 |
| MAIL-04 | Agent can read full message content via API | `GET /api/mail/messages/{uid}` 返回 `MessageDetail`（含 text_body, html_body, attachments）。`GET /api/conversations/{thread_id}` 返回 thread 内所有消息含全文。**已可直接使用** |
| MAIL-05 | Agent can list conversations and search messages via API | `GET /api/conversations` + `GET /api/conversations/search` 已存在，支持 limit/before/category/folder 过滤和文本+语义搜索。**已可直接使用** |
| MAIL-06 | Agent can reply to existing thread via API | `send_message` 已支持 `in_reply_to` 字段（Message-ID），自动构建 References 链和引用原文。需为 agent 提供更友好的入口：通过 thread_id 而非 Message-ID 回复 |
</phase_requirements>

## Standard Stack

### Core (all existing, no new dependencies)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| axum | 0.8 | HTTP framework + Multipart | 已在用，route 和 extractor 直接复用 |
| sqlx | 0.8 | PostgreSQL queries | 运行时 query，已在用 |
| mail-builder | (existing) | RFC 5322 message construction | `build_rfc5322_message` 已在用 |
| mail-parser | (existing) | MIME parsing, attachment extraction | `parse_message` 已在用 |
| serde/serde_json | (existing) | Request/response serialization | 已在用 |

### Supporting (existing)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| base64 | (existing) | Attachment encoding | multipart 附件处理已在用 |
| rand_core | (existing) | Message-ID generation | 已在用 |
| chrono | (existing) | Timestamp formatting | reply 引用头已在用 |

**Installation:** 无需安装任何新依赖。

## Architecture Patterns

### Pattern 1: Reuse Existing Endpoints (NOT Duplicate)

Phase 1 的统一 AuthUser extractor 是关键。所有现有端点自动获得 API key 支持：

```
Agent request with API key
    → AuthUser extractor (already handles mlrs_ prefix)
    → Existing handler (send_message, get_message, etc.)
    → Same response
```

**不要** 创建 `/api/agent/mail/*` 路由复制现有逻辑。

### Pattern 2: Superadmin From Address Validation

当前 `send_message` 硬拒绝 from != user：

```rust
// 当前逻辑（mail.rs line ~439-444）
if from != &user {
    return Json(ApiResult {
        success: false,
        message: Some("sender must match authenticated user".into()),
    });
}
```

需改为：

```rust
// 新逻辑：superadmin 可以指定其管辖 domain 的任意 from
if from != &user {
    let from_domain = from.split('@').nth(1).unwrap_or("");
    if super_domains.is_empty() || !super_domains.iter().any(|d| d == from_domain) {
        return Json(ApiResult {
            success: false,
            message: Some("sender must match authenticated user or be in super_domains".into()),
        });
    }
}
```

### Pattern 3: Thread-Based Reply (Agent-Friendly)

现有 `in_reply_to` 需要调用者知道原始邮件的 Message-ID。对 agent 不友好。两种方案：

**方案 A（推荐）：扩展 SendMessageRequest 增加 `reply_to_thread_id` 字段**
- Agent 通过 conversations API 获取 thread_id
- send_message 内部查找 thread 最后一条消息的 Message-ID
- 自动设置 In-Reply-To 和 References

**方案 B：新增 `/api/conversations/{thread_id}/reply` 端点**
- 语义更清晰，但会引入新路由

推荐方案 A：改动最小，复用现有 send 逻辑。SaveDraftRequest 已有 `reply_to_thread_id` 字段可参考。

### Anti-Patterns to Avoid

- **重复端点:** 不要为 agent 创建平行的 mail API
- **Base64 附件 in JSON:** 已在 Out of Scope 中明确排除，用 multipart/form-data
- **忽略 super_domains 校验:** superadmin from 必须验证 domain 在授权范围内

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MIME message construction | 手写 RFC 5322 | `build_rfc5322_message` (existing) | 已有完整实现含 attachments、References、MIME multipart |
| Thread reference chain | 手动查询 Message-ID 链 | `mb_store.get_thread_references()` | 已有实现，正确处理 References header |
| Message parsing | 手写 MIME parser | `message_util::parse_message` | mail-parser 已处理所有 MIME 类型 |

## Common Pitfalls

### Pitfall 1: send_message_multipart 的 from 校验缺失
**What goes wrong:** multipart 版本可能有与 JSON 版本不同的 from 校验逻辑
**How to avoid:** 确保 superadmin 校验同时应用于 `send_message` 和 `send_message_multipart`

### Pitfall 2: reply_to_thread_id 查找错误的 Message-ID
**What goes wrong:** thread 中最后一条消息可能是自己发的，导致 In-Reply-To 指向自己
**How to avoid:** 查找 thread 中最后一条 **非当前用户发送** 的消息，或者简单取 thread 最后一条消息（标准邮件行为）

### Pitfall 3: get_message 的 UID 跨用户不唯一
**What goes wrong:** superadmin 用 domains 参数查看其他用户邮件时，UID 可能冲突
**How to avoid:** 对 agent 场景，推荐使用 conversations API（thread_id 全局唯一）而非 mail/messages/{uid}

### Pitfall 4: Multipart form-data 字段顺序
**What goes wrong:** axum Multipart 按流式读取，metadata 字段必须在 file 字段之前
**How to avoid:** 文档明确说明字段顺序要求，或先收集所有字段再处理

## Code Examples

### Existing send_message handler structure (verified from mail.rs)

```rust
// SendMessageRequest 已支持:
pub struct SendMessageRequest {
    pub from: String,          // MAIL-03: 需放开 superadmin 限制
    pub to: Vec<String>,       // MAIL-01: ok
    pub cc: Vec<String>,       // MAIL-01: ok
    pub bcc: Vec<String>,      // MAIL-01: ok
    pub subject: String,       // MAIL-01: ok
    pub body: String,          // MAIL-01: ok (text)
    pub html_body: Option<String>,  // MAIL-01: ok
    pub in_reply_to: Option<String>, // MAIL-06: 现有，需增加 thread_id 替代
    pub list_unsubscribe: Option<String>,
}
```

### Existing AuthUser (from Phase 1)

```rust
pub(crate) struct AuthUser {
    pub address: String,
    pub display_name: String,
    pub super_domains: Vec<String>,  // MAIL-03: 已有，用于 from 校验
    pub auth_method: AuthMethod,     // Session | ApiKey(i64)
}
```

### Existing conversation search (already works for agents)

```
GET /api/conversations?limit=20&category=personal&folder=INBOX
GET /api/conversations/search?q=invoice&limit=10
GET /api/conversations/{thread_id}  → returns all messages with full body
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Agent 专用路由 | 统一 AuthUser，复用现有路由 | Phase 1 完成 | 无需新端点，只调整逻辑 |
| from 硬校验 | super_domains 条件放开 | Phase 2 需实现 | MAIL-03 核心改动 |
| Message-ID reply | thread_id reply | Phase 2 需实现 | MAIL-06 agent 友好性 |

## Open Questions

1. **send_message_multipart 的完整实现**
   - What we know: 路由存在 `POST /api/mail/send-multipart`，有 25MB 限制
   - What's unclear: 完整实现未在当前读取范围内（文件太长），需确认 attachments 处理逻辑
   - Recommendation: 实现阶段需完整审查 `send_message_multipart` handler

2. **Superadmin 是否应该能代发任意 domain（不仅限 super_domains）**
   - What we know: super_domains 是 account 级别配置，通常包含管辖的 domain 列表
   - What's unclear: 是否需要支持发送到非本地 domain（如 superadmin 代发 gmail.com 的地址）
   - Recommendation: 限制为 super_domains 内的 domain，安全性更高

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p mailrs-server -- --test-threads=1` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MAIL-01 | send_message accepts API key auth | integration | `cargo test -p mailrs-server send_message` | Needs new tests |
| MAIL-02 | send_message_multipart with attachments | integration | `cargo test -p mailrs-server send_message_multipart` | Needs new tests |
| MAIL-03 | superadmin from address validation | unit | `cargo test -p mailrs-server validate_from` | Wave 0 |
| MAIL-04 | get_message returns full content | integration | existing endpoints already tested via web UI | Needs agent-specific test |
| MAIL-05 | list/search conversations | integration | existing endpoints already tested via web UI | Needs agent-specific test |
| MAIL-06 | reply via thread_id | unit + integration | `cargo test -p mailrs-server reply_thread` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p mailrs-server`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Unit tests for superadmin from-address validation logic (MAIL-03)
- [ ] Unit tests for thread_id to Message-ID resolution (MAIL-06)
- [ ] Integration test helper for creating AuthUser with API key (shared fixture)

## Sources

### Primary (HIGH confidence)
- mailrs codebase: `crates/server/src/web/mail.rs` — SendMessageRequest, send_message, send_message_multipart, get_message
- mailrs codebase: `crates/server/src/web/conversations.rs` — get_conversations, search_conversations, get_thread_messages
- mailrs codebase: `crates/server/src/web/auth.rs` — AuthUser with super_domains, API key verification
- mailrs codebase: `crates/server/src/web/mod.rs` — all route definitions, constants (MAX_RECIPIENTS, MAX_MULTIPART_BODY, etc.)
- mailrs codebase: `crates/server/src/api_key_store.rs` — API key generation, verification, Valkey cache

### Secondary (MEDIUM confidence)
- `.planning/research/ARCHITECTURE.md` — component boundaries, anti-patterns
- `.planning/research/STACK.md` — technology decisions
- `.planning/REQUIREMENTS.md` — requirement definitions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all existing dependencies, zero new crates
- Architecture: HIGH — endpoints already exist, verified from source code
- Pitfalls: HIGH — identified from actual code review (from validation, UID uniqueness, multipart ordering)

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable, internal project)
