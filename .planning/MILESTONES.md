# Milestones

## v1.0 AI Agent API (Shipped: 2026-03-09)

**Phases completed:** 4 phases, 8 plans, 0 tasks

**Timeline:** 8 days (2026-03-02 → 2026-03-10)
**Stats:** 55 files changed, +7,464 / -307 lines

**Key accomplishments:**
1. API key 认证系统 — `mlrs_` 前缀 + SHA-256 哈希存储 + Valkey 缓存 + CRUD 端点 + 权限继承
2. Agent 邮件操作 — superadmin from 覆盖 + reply_to_thread_id + 15 个集成测试验证端点完备
3. Webhook 订阅系统 — DB outbox 模式 + HMAC-SHA256 签名 + 指数退避重试 + Semaphore 并发控制
4. MCP Server — rmcp 1.1 嵌入 Axum + 5 个邮件工具 + task-local auth 传播 + Claude Code 端到端验证
5. 2 个生产 bug 修复 — inline 图片 CID 转换 (v0.6.26) + MCP auth identity 传播 (v0.6.28)

---

