# Technology Stack

**Project:** mailrs AI Agent API
**Researched:** 2026-03-09

## Recommended Stack

### Decision Change: MCP Server in Rust, Not TypeScript

PROJECT.md 中预设了 "MCP server 用 TypeScript"，但研究结论是 **应该用 Rust**。理由：

1. **rmcp 1.1.0 已成熟** — 官方 Rust MCP SDK 已达 1.x 稳定版，支持 `#[tool]` 宏、Streamable HTTP transport、与 Axum 原生集成
2. **消除独立进程** — TypeScript MCP server 需要单独部署和维护，Rust 版直接嵌入 mailrs-server 进程，共享 `WebState`（pg_pool, mailbox_store, event_bus）
3. **无 HTTP 中间层** — TypeScript 方案需要 MCP server -> HTTP -> mailrs REST API 的中间调用，Rust 方案直接调用内部函数，延迟更低、无认证绕过风险
4. **统一技术栈** — 不引入 Node.js 运行时依赖到部署流程
5. **类型安全** — 工具参数通过 Rust 类型系统和 serde 校验，编译期捕获错误

**Confidence: HIGH** — rmcp 1.1.0 在 crates.io 有文档，Shuttle 等有完整 Axum 集成教程。

### Core: API Key Authentication (Rust, 内嵌 mailrs-server)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| axum | 0.8 (existing) | HTTP framework | 已在用，API key 路由直接加入现有 router |
| argon2 | 0.5 (existing) | API key hashing | 已在用于密码 hash，API key 同样需要单向 hash 存储 |
| sha2 + hex | 0.10 / 0.4 (existing) | API key prefix hash | 用 SHA-256 做 key 前缀的快速查找索引（prefix -> full key hash） |
| rand_core | 0.6 (existing) | API key generation | 已在用于 session token 生成，复用 `OsRng` |
| sqlx | 0.8 (existing) | API key 持久化 | 新建 `api_keys` 表，存 key hash + 关联 account + 权限 + 过期时间 |

**Confidence: HIGH** — 全部是现有依赖，无新 crate 引入。

### Core: MCP Server (Rust, 内嵌 mailrs-server)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| rmcp | 1.1 | MCP protocol SDK | 官方 Rust SDK，支持 `#[tool]` 宏声明工具、`#[tool_handler]` 自动路由 |
| rmcp (feature: server) | 1.1 | Server-side MCP | 服务端工具注册和处理 |
| rmcp (feature: transport-streamable-http-server) | 1.1 | Streamable HTTP transport | 通过 HTTP 暴露 MCP 端点，支持 SSE 流式响应 |
| rmcp (feature: macros) | 1.1 | `#[tool]` 等宏 | 减少 boilerplate，声明式定义工具 |

**Confidence: MEDIUM** — rmcp 1.1.0 存在且有文档，但 mailrs 尚未实际引入，需要验证与现有 Axum 0.8 的兼容性。rmcp 默认使用 axum，应兼容。

### Core: Webhook System (Rust, 内嵌 mailrs-server)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| reqwest | 0.12 (existing) | Webhook HTTP 推送 | 已在用，无需新增依赖 |
| backon | 1.6 | 指数退避重试 | 比 backoff crate 更现代（backoff 已停止维护），API 设计贴合 Rust async，支持 jitter 和动态退避 |
| hmac + sha2 | existing sha2 | Webhook 签名 | HMAC-SHA256 签名验证，sha2 已在依赖中，只需加 hmac crate |

**Confidence: HIGH** — reqwest 已在用，backon 是当前 Rust 生态中推荐的 retry crate（1.6.0 stable）。

### Database Schema Extensions

| Table | Purpose | Key Columns |
|-------|---------|-------------|
| `api_keys` | API key 存储 | `id`, `account_address`, `key_prefix` (6 char), `key_hash` (argon2), `name`, `permissions`, `expires_at`, `created_at`, `last_used_at`, `revoked` |
| `webhook_subscriptions` | Webhook 订阅 | `id`, `account_address`, `url`, `secret` (encrypted), `filter_type` (contact/thread/all), `filter_value`, `active`, `created_at` |
| `webhook_deliveries` | 投递日志 | `id`, `subscription_id`, `event_type`, `payload`, `status_code`, `attempts`, `next_retry_at`, `created_at` |

**Confidence: HIGH** — 标准 schema 设计，与现有 PostgreSQL + sqlx 模式一致。

### Supporting Libraries (New)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rmcp | 1.1 | MCP server SDK | MCP 工具注册和协议处理 |
| backon | 1.6 | Retry with backoff | Webhook 投递失败重试 |
| hmac | 0.12 | HMAC 签名 | Webhook payload 签名（HMAC-SHA256） |

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| MCP SDK | rmcp (Rust) | @modelcontextprotocol/sdk (TypeScript) | 需要独立进程 + HTTP 中间层，增加部署和维护复杂度 |
| MCP SDK | rmcp (official) | rust-mcp-sdk (community) | 官方 SDK 更稳定，Anthropic 直接维护 |
| Retry | backon | backoff | backoff 已停止维护，backon 是推荐替代 |
| API key hash | argon2 (existing) | bcrypt | argon2 已在项目中，更现代且抗 GPU/ASIC |
| Webhook signing | HMAC-SHA256 | Ed25519 | HMAC-SHA256 是行业标准（Stripe, GitHub 都用），接收方实现更简单 |
| MCP transport | Streamable HTTP | stdio | stdio 需要 Claude Code 在本地 spawn 进程，不适合远程服务器场景 |

## MCP Transport Strategy

**同时支持 stdio 和 Streamable HTTP**：

1. **Streamable HTTP** (primary) — 嵌入 mailrs-server 的 Axum router，路径 `/mcp`。远程 Claude Code 通过 `claude mcp add --transport http` 连接
2. **stdio wrapper** (secondary) — 一个轻量 Rust binary（`mailrs-mcp`），启动后连接 mailrs REST API，转发为 stdio。用于本地开发场景

Claude Code 从 2025 年中开始支持 Streamable HTTP remote MCP servers，优先使用此方式。

## Installation

```toml
# Cargo.toml (server crate) — new dependencies only
[dependencies]
rmcp = { version = "1.1", features = ["server", "macros", "transport-streamable-http-server"] }
backon = "1.6"
hmac = "0.12"
```

现有依赖无需变更：axum 0.8, sqlx 0.8, reqwest 0.12, argon2 0.5, sha2 0.10, rand_core 0.6 均已满足需求。

## Sources

- [rmcp on crates.io](https://crates.io/crates/rmcp) — v1.1.0, official Rust MCP SDK
- [rmcp on docs.rs](https://docs.rs/crate/rmcp/latest) — API documentation
- [Shuttle: Streamable HTTP MCP Server in Rust](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp) — Axum integration tutorial
- [modelcontextprotocol/rust-sdk on GitHub](https://github.com/modelcontextprotocol/rust-sdk) — official repository
- [@modelcontextprotocol/sdk on npm](https://www.npmjs.com/package/@modelcontextprotocol/sdk) — v1.27.1, TypeScript alternative (not recommended)
- [backon on crates.io](https://crates.io/crates/backon) — v1.6.0, retry with backoff
- [Claude Code MCP docs](https://code.claude.com/docs/en/mcp) — Claude Code MCP integration
- [Claude Code remote MCP support](https://www.infoq.com/news/2025/06/anthropic-claude-remote-mcp/) — Streamable HTTP support announcement
- [MCP Transports spec](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) — stdio vs Streamable HTTP
