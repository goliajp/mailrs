# mailrs AI Agent API

## What This Is

mailrs 的 AI agent 集成层 — 通过 API key 认证的 REST API 和 MCP server，让 AI agent（特别是 Claude Code）能够发送邮件（含附件）、读取邮件、搜索会话、回复 thread、订阅新邮件 webhook 通知。构建在 mailrs 现有的邮件基础设施之上，已投入生产使用。

## Core Value

AI agent 能通过简单的 API 调用收发邮件，像人类用邮箱一样自然地参与邮件通信。

## Requirements

### Validated

- ✓ API key CRUD + SHA-256 哈希 + Valkey 缓存 + 过期 + revoke — v1.0
- ✓ Bearer mlrs_ 认证接入 auth extractor，权限继承账号角色 — v1.0
- ✓ Superadmin key 可操控任意邮箱 — v1.0
- ✓ Agent 发送邮件（含附件、from 指定、thread 回复） — v1.0
- ✓ Agent 读取邮件全文、列出会话、搜索消息 — v1.0
- ✓ Webhook 订阅（按联系人/thread 过滤）+ HMAC-SHA256 签名 + DB outbox + 指数退避 — v1.0
- ✓ MCP server 嵌入 mailrs-server（rmcp 1.1 + Streamable HTTP /mcp） — v1.0
- ✓ 5 个 MCP 工具（send/read/search/reply/list_conversations） — v1.0

### Active

(Empty — define next milestone requirements via `/gsd:new-milestone`)

### Out of Scope

- OAuth 2.0 授权 — 服务间调用场景，API key 足够，复杂度不值得
- GraphQL API — REST 足够，维护成本高
- Webhook 推邮件全文 — Payload 大、敏感数据暴露风险
- MCP marketplace 发布 — 先满足自用，稳定后再考虑
- 邮件 AI 分析/摘要 — 已有 ai_assist 模块，不在本次范围
- Base64 附件 in JSON body — 内存爆炸风险，用 multipart/form-data

## Context

v1.0 已上线。46,917 LOC Rust + 13,679 LOC TypeScript。
Tech stack: Axum + Tokio + sqlx/PostgreSQL + Valkey + rmcp 1.1。
Claude Code 已通过 MCP 实际收发邮件，验证端到端可用。

v2 候选功能：per-key scopes、rate limiting per key、webhook delivery log、MCP stdio wrapper、MCP resources。

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| API key 而非 OAuth | 服务间调用场景，简单直接 | ✓ Good — 接入成本极低 |
| Webhook 只推 message ID | 减少 payload，agent 按需拉全文 | ✓ Good — 安全且灵活 |
| MCP server 用 Rust (rmcp) 嵌入 | 避免 TypeScript 独立进程，减少运维复杂度 | ✓ Good — 单进程部署 |
| API key 权限继承账号角色 | 复用现有 accounts 表权限 | ✓ Good — 零额外权限系统 |
| DB outbox 而非 EventBus 直接推 | EventBus broadcast 会 lag 丢事件 | ✓ Good — 可靠不丢 |
| MCP router 在 rate limiter 前合并 | 避免限流长连接 MCP session | ✓ Good — MCP 体验流畅 |
| verify_sender 提取为 pub(crate) pure function | 可测性 + 复用性 | ✓ Good — MCP 复用 |
| task-local 传播 auth identity 到 MCP service | factory 无法拿到 per-request auth | ✓ Good — 修复 v0.6.28 |

## Constraints

- **Tech stack**: Rust (server) — MCP 也用 Rust (rmcp 1.1)
- **Auth**: API key 必须支持 revoke 和过期时间
- **Security**: API key 存储必须 hash，不能明文存数据库；webhook signing_secret 明文存储（HMAC 需要原始密钥）
- **Compatibility**: 新 API 不破坏现有 web UI 的 session auth

---
*Last updated: 2026-03-10 after v1.0 milestone*
