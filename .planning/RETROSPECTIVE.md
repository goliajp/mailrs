# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — AI Agent API

**Shipped:** 2026-03-10
**Phases:** 4 | **Plans:** 8 | **Sessions:** ~3

### What Was Built
- API key 认证系统 (mlrs_ 前缀, SHA-256, Valkey 缓存, CRUD, 过期/revoke)
- Agent 邮件操作 (superadmin from 覆盖, thread 回复, 15 个集成测试)
- Webhook 订阅系统 (DB outbox, HMAC-SHA256 签名, 指数退避, Semaphore 并发)
- MCP Server (rmcp 1.1 嵌入 Axum, 5 个工具, task-local auth)

### What Worked
- 纯函数提取模式 (verify_sender, matches_subscription) — 极大提升可测性和跨 phase 复用
- Phase 3/4 并行执行 — 依赖分析准确，两个 phase 同时推进无冲突
- 现有基础设施复用 — 大量端点已 agent-ready，Phase 2 测试验证无需改动
- DB outbox 而非 EventBus 直接推 — 从设计上消除了事件丢失风险

### What Was Inefficient
- PROJECT.md 的 Constraints 一开始写了 "MCP server 用 TypeScript"，实际用了 Rust — 前期研究不够深
- ROADMAP.md 中 Phase 2/3/4 的 plan checkbox 没有随执行更新（archive 时仍显示 unchecked）
- STATE.md 多次追加 frontmatter 导致文件格式混乱（多个 `---` block）

### Patterns Established
- `pub(crate)` pure function 模式 — 核心逻辑提取为无 I/O 纯函数，handler/service 层调用
- DB outbox + polling worker — webhook/通知类场景的标准模式
- task-local auth 传播 — 解决 factory 无法获取 per-request context 的问题
- MCP router 在 rate limiter 前合并 — 长连接协议的标准路由策略

### Key Lessons
1. MCP 生态变化快 — rmcp 1.1 刚发布就比 TypeScript SDK 更适合嵌入，技术选型要做实际 POC 而非凭印象
2. 现有端点比想象中更 agent-ready — 开始前先跑测试验证，避免不必要的改动
3. task-local 是 Rust async 中传播 per-request context 到 factory-created service 的好模式

### Cost Observations
- Model mix: 主要 opus，部分 sonnet
- Sessions: ~3 sessions over 8 days
- Notable: 8 个 plan 平均 6min/plan，总执行约 45min — 规划+研究时间远大于执行时间

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Sessions | Phases | Key Change |
|-----------|----------|--------|------------|
| v1.0 | ~3 | 4 | First milestone — established GSD workflow |

### Cumulative Quality

| Milestone | Tests | Coverage | Zero-Dep Additions |
|-----------|-------|----------|-------------------|
| v1.0 | 15+ integration | partial | 2 (hmac, rmcp+schemars) |

### Top Lessons (Verified Across Milestones)

1. Pure function extraction enables both testability and cross-module reuse
2. Always verify existing code before assuming it needs changes
