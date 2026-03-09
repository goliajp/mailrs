# mailrs AI Agent API

## What This Is

mailrs 的 AI agent 集成层 — 通过 API key 认证的 REST API 和 MCP server，让 AI agent（特别是 Claude Code）能够发送邮件（含附件）、读取邮件、订阅特定联系人或 thread 的新邮件通知。构建在 mailrs 现有的邮件基础设施之上。

## Core Value

AI agent 能通过简单的 API 调用收发邮件，像人类用邮箱一样自然地参与邮件通信。

## Requirements

### Validated

<!-- 已有基础设施 -->

- ✓ SMTP 收发邮件 — existing
- ✓ IMAP 协议支持 — existing
- ✓ Web UI 邮件管理 — existing
- ✓ REST API 发送邮件 (`/api/mail/send`) — existing
- ✓ REST API 读取邮件 (`/api/mail/messages/{uid}`) — existing
- ✓ Conversations/thread 管理 — existing
- ✓ PostgreSQL 账号/域名管理 — existing
- ✓ Session-based auth (login + bearer token) — existing

### Active

- [ ] API key 认证系统
- [ ] API key 绑定账号，权限继承账号角色
- [ ] Superadmin key 可操控任意邮箱
- [ ] 通过 API 发送邮件（含附件，可指定 from 地址）
- [ ] 通过 API 读取邮件全文
- [ ] 创建 webhook 订阅（按联系人过滤）
- [ ] 创建 webhook 订阅（按 thread/reply 过滤）
- [ ] Webhook 推送通知（推送 message ID，agent 自行拉全文）
- [ ] MCP server 包装 REST API
- [ ] Claude Code 可通过 MCP 直接收发邮件

### Out of Scope

- OAuth 2.0 授权 — 当前场景是服务间调用，API key 足够，OAuth 复杂度不值得
- 邮件内容的 AI 分析/摘要 — 已有 ai_assist 模块，不在本次范围
- 第三方 MCP marketplace 发布 — 先满足自用

## Context

- mailrs 已有完整的 web API（Axum），新 API 可以复用现有路由和中间件
- 账号系统已有 role 概念（accounts 表），API key 权限可以直接继承
- event_bus 已有 `SmtpEvent` 广播机制，webhook 可以订阅这个 bus
- 现有 `/api/mail/send` 已支持发邮件，需要扩展支持附件和 from 指定
- MCP server 是独立进程，通过 HTTP 调用 mailrs REST API

## Constraints

- **Tech stack**: Rust (server) + TypeScript (MCP server) — MCP SDK 生态 TypeScript 最成熟
- **Auth**: API key 必须支持 revoke 和过期时间
- **Security**: API key 存储必须 hash（类似密码），不能明文存数据库
- **Compatibility**: 新 API 不能破坏现有 web UI 的 session auth

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| API key 而非 OAuth | 服务间调用场景，简单直接，接入成本低 | — Pending |
| Webhook 只推 message ID | 减少 payload 大小，agent 按需拉全文，避免敏感数据在 webhook 中传输 | — Pending |
| MCP server 用 TypeScript | MCP SDK 官方 TypeScript 支持最好，且 mailrs 已有 TypeScript 前端生态 | — Pending |
| API key 权限继承账号角色 | 复用现有 accounts 表的权限模型，不另建权限系统 | — Pending |

---
*Last updated: 2026-03-09 after initialization*
