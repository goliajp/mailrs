# Feature Landscape

**Domain:** AI Agent Email API (REST + MCP + Webhooks)
**Researched:** 2026-03-09 (updated with stack research)
**Confidence:** MEDIUM-HIGH

## Table Stakes

Features AI agents and developers expect. Missing = API is unusable.

### API Key Management

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| API key generation (per account) | 基本认证方式，没有就无法使用 API | Low | 每个 account 可创建多个 key |
| API key revocation | 泄露后必须能立即失效 | Low | 软删除 + Valkey 立即清除 |
| API key hashing (Argon2) | 安全底线，明文存储不可接受 | Low | 只展示一次完整 key，存 SHA-256 hash |
| Key prefix identification | 方便用户识别哪个 key | Low | 存储 prefix（前 8 位）+ hash |
| Bearer token auth | 行业标准 `Authorization: Bearer <key>` | Low | 与现有 session auth 并存于同一 extractor |
| Key expiration time | 限制泄露窗口 | Low | 可选，默认不过期 |

### Email Operations

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Send email (to/cc/bcc, subject, body) | 核心功能 | Low | 现有 `/api/mail/send` 加 API key auth |
| HTML + plain text body | 标配 | Low | multipart/alternative |
| Attachment support | Agent 发送文件 | Med | 现有 `send-multipart` 扩展 |
| From address control (superadmin) | 代发不同账号邮件 | Med | 普通 key 只能用绑定账号地址 |
| Get message by ID | 基本读取 | Low | 现有 API |
| List conversations | 浏览邮箱 | Low | 现有 conversations API |
| Search messages | 找到特定邮件 | Med | 现有 search + semantic search |
| Reply to thread | 参与已有对话 | Med | In-Reply-To/References headers |

### MCP Tools

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| send_email tool | MCP 核心 | Low | 用 rmcp `#[tool]` 宏包装内部函数 |
| read_email tool | 读取邮件 | Low | 直接调用 MailboxStore |
| list_conversations tool | 浏览对话 | Low | 直接调用 conversations 查询 |
| search_emails tool | 搜索邮件 | Low | 包装现有搜索逻辑 |
| reply_email tool | 回复邮件 | Low | 包装发送 + 设置 reply headers |

### Webhook Notifications

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Register webhook URL | 实时通知基础 | Med | CRUD endpoints |
| Event type filtering | 只关心特定事件 | Low | `new_message` 为主 |
| Webhook HMAC signing | 验证请求来源 | Low | HMAC-SHA256（hmac crate） |
| Exponential backoff retry | 不丢事件 | Med | backon 1.6 |
| Lightweight payload (只推 ID) | 安全 + 性能 | Low | PROJECT.md 已决定 |

## Differentiators

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Webhook 按联系人过滤 | Agent 只关注特定人 | Med | subscription 绑定 contact email |
| Webhook 按 thread 过滤 | Agent 追踪特定对话 | Med | subscription 绑定 thread_id |
| Superadmin API key | 运维操控任意邮箱 | Med | 继承 account super_domains |
| MCP Streamable HTTP 内嵌 | 无需独立进程，远程可用 | Med | rmcp + axum，`/mcp` 端点 |
| MCP resources: mailbox_summary | 快速了解邮箱状态 | Low | unread count + recent |
| Webhook delivery log | 查看推送历史 | Med | webhook_deliveries 表 |
| Idempotency key | 消费方去重 | Low | 每个事件带 delivery_id |

## Anti-Features

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| OAuth 2.0 | 服务间调用，复杂度不值得 | API key + revoke |
| 邮件 AI 分析 | 已有 ai_assist 模块 | 保持独立模块 |
| MCP marketplace 发布 | 先自用 | 稳定后再考虑 |
| 复杂 webhook 过滤 DSL | 过度设计 | contact + thread 两种 |
| Webhook 推邮件全文 | payload 大、敏感数据 | 推 message ID |
| 独立 TypeScript MCP 进程 | 增加部署复杂度 + HTTP 中间层 | Rust rmcp 嵌入 mailrs-server |
| GraphQL API | 维护成本高 | REST + 合理的 field selection |
| Base64 附件 in JSON | 内存爆炸 | multipart/form-data |

## Feature Dependencies

```
API Key System (generation, auth middleware)
  |
  +---> Email Send API (attachment, from control)
  |       +---> Reply to Thread
  |
  +---> Email Read API (get, list, search)
  |       +---> Get Attachment
  |
  +---> Webhook System
  |       +---> Event Filtering (contact, thread)
  |       +---> Retry + Delivery Log
  |
  +---> MCP Server (shares WebState, calls internal functions)
          +---> Tool definitions via #[tool] macros
          +---> Streamable HTTP on /mcp route
```

**Critical path:** API Key System -> REST APIs -> MCP Server + Webhook (可并行)

注意：因为 MCP server 现在用 Rust 嵌入，它不再依赖 REST API 稳定才能开始。MCP tools 直接调用内部函数，可以与 REST API 并行开发。Webhook 同样独立于 MCP。

## MVP Recommendation

优先级排序：

1. **API key 认证** — 一切前提
2. **Agent REST API** — 复用现有逻辑加 API key auth layer
3. **MCP server (Streamable HTTP)** — 嵌入 Axum，可与 Phase 2 并行
4. **Webhook 订阅 + 投递** — 可与 Phase 3 并行

延期：
- **stdio MCP wrapper binary** — Streamable HTTP 已覆盖
- **Webhook 高级过滤** — 先 contact + thread
- **Per-key 细粒度 scopes** — 先继承 account role
- **Template / scheduled send** — agent 可自行组织

## Sources

- mailrs PROJECT.md
- mailrs codebase (web/mod.rs, auth.rs, event_bus.rs)
- [rmcp on crates.io](https://crates.io/crates/rmcp) — v1.1.0
- [backon on crates.io](https://crates.io/crates/backon) — v1.6.0
