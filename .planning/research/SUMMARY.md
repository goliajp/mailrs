# Research Summary: mailrs AI Agent API

**Domain:** AI agent email API (API key auth + MCP server + webhooks)
**Researched:** 2026-03-09
**Overall confidence:** MEDIUM-HIGH

## Executive Summary

mailrs 已有完整的邮件基础设施（SMTP 收发、IMAP、Web UI、REST API、PostgreSQL、Valkey）。本研究聚焦在此基础上添加 AI agent 集成层所需的技术栈。

核心发现：**MCP server 应该用 Rust（rmcp 1.1）嵌入 mailrs-server 进程，而非 PROJECT.md 中预设的 TypeScript 独立进程。** rmcp 官方 SDK 已达 1.x 稳定版，原生支持 Axum 集成和 Streamable HTTP transport。嵌入方案消除了独立进程的部署开销、HTTP 中间层延迟、以及 API 漂移风险。

API key 认证系统可完全复用现有依赖（axum 0.8, argon2 0.5, sqlx 0.8, sha2 0.10），仅需扩展 `AuthUser` extractor 识别 `mlrs_` 前缀的 token。Webhook 系统同样复用现有 EventBus（tokio broadcast）和 reqwest，新增 backon 1.6 做指数退避重试。

最大的技术风险是 rmcp 1.1 与现有 axum 0.8 的兼容性（需实际 cargo check 验证），以及 MCP tool 的 prompt injection 风险（邮件内容通过 tool response 到达 AI agent）。

## Key Findings

**Stack:** 全部用 Rust，3 个新 crate（rmcp 1.1, backon 1.6, hmac 0.12），其余复用现有依赖
**Architecture:** 三个新子系统嵌入现有 mailrs-server 进程，共享 WebState 和 PG/Valkey
**Critical pitfall:** EventBus broadcast channel lag 会丢 webhook 事件 — 必须用 DB outbox 解耦 capture 和 delivery

## Implications for Roadmap

基于研究，建议 phase 结构：

1. **Phase 1: API Key Authentication** — 一切基础
   - Addresses: api_keys 表 + AuthUser extractor 扩展 + CRUD endpoints
   - Avoids: cache staleness (Valkey 立即删除 on revoke)

2. **Phase 2: Agent REST API Enhancement** — 补齐发送附件、from 控制等
   - Addresses: 现有 REST API 加 API key auth，扩展 send-multipart
   - Avoids: base64 内存爆炸 (用 multipart/form-data)

3. **Phase 3: MCP Server (Streamable HTTP)** — 可与 Phase 4 并行
   - Addresses: rmcp 嵌入 Axum，`#[tool]` 宏定义工具，/mcp 端点
   - Avoids: prompt injection (内容分隔符 + text_body only)

4. **Phase 4: Webhook System** — 可与 Phase 3 并行
   - Addresses: 订阅 CRUD + EventBus capture + async delivery + retry
   - Avoids: broadcast lag (DB outbox pattern)

**Phase ordering rationale:**
- Phase 1 是硬依赖 — Phase 2/3/4 都需要 API key auth
- Phase 2 先于 3/4 因为 MCP tools 和 webhook 都需要底层 mail 功能完善
- Phase 3 和 4 可并行：MCP 直接调用内部函数，不依赖 webhook；webhook 不依赖 MCP

**Research flags for phases:**
- Phase 3 (MCP): 需要验证 rmcp 1.1 + axum 0.8 编译兼容性
- Phase 1/2/4: 标准模式，不太需要额外研究

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | API key 和 webhook 全部复用现有依赖；rmcp 1.1 有文档和教程 |
| Features | HIGH | 需求明确（PROJECT.md），竞品参考充分 |
| Architecture | MEDIUM-HIGH | 嵌入方案架构清晰，rmcp + axum 兼容性是唯一未验证点 |
| Pitfalls | HIGH | 基于实际代码分析 + 行业案例（MCP prompt injection, broadcast lag） |

## Gaps to Address

- **rmcp + axum 0.8 兼容性** — 需要实际 `cargo check` 验证，目前是 MEDIUM confidence
- **MCP 认证集成** — rmcp Streamable HTTP 如何传递 API key 待验证（可能通过 HTTP header 或 query param）
- **Webhook auto-disable 策略** — 连续失败多少次后暂停、恢复机制，需要实际运营经验
- **rmcp #[tool] 宏的限制** — 参数类型支持范围、错误处理模式，需要在实现阶段深入研究
