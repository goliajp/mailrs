# Roadmap: mailrs AI Agent API

## Overview

在 mailrs 现有邮件基础设施上构建 AI agent 集成层。从 API key 认证开始（所有后续功能的基础），然后补齐 agent 邮件操作能力，最后并行交付 webhook 通知系统和 MCP server。四个 phase 对应四个自然的交付边界：认证 → 邮件操作 → 异步通知 → AI 工具协议。

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3, 4): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: API Key Authentication** - API key CRUD、哈希存储、Bearer 认证、权限继承 (completed 2026-03-09)
- [x] **Phase 2: Agent Email Operations** - 发送（含附件）、读取、搜索、回复邮件的 REST API (completed 2026-03-09)
- [ ] **Phase 3: Webhook Subscriptions** - 订阅管理、事件捕获（DB outbox）、异步投递、重试
- [ ] **Phase 4: MCP Server** - rmcp 嵌入 Axum、Streamable HTTP、邮件工具集

## Phase Details

### Phase 1: API Key Authentication
**Goal**: Agent 能通过 API key 认证访问 mailrs API，权限继承账号角色
**Depends on**: Nothing (first phase)
**Requirements**: AKEY-01, AKEY-02, AKEY-03, AKEY-04, AKEY-05, AKEY-06, AKEY-07
**Success Criteria** (what must be TRUE):
  1. User 可在 Web UI 或 API 创建 API key，密钥仅在创建时显示一次
  2. Agent 使用 `Authorization: Bearer mlrs_...` 请求任意已有 API 端点，认证通过并获得对应账号权限
  3. Superadmin key 可以操作任意邮箱的端点
  4. User revoke API key 后，该 key 立即失效（含 Valkey 缓存清除），后续请求返回 401
  5. 设置了过期时间的 API key 到期后自动失效
**Plans**: 2 plans

Plans:
- [x] 01-01-PLAN.md — AuthUser 重构为 named-field struct + api_keys 表迁移 + api_key_store 存储模块
- [x] 01-02-PLAN.md — API key 认证接入 auth extractor + CRUD 端点 + 测试

### Phase 2: Agent Email Operations
**Goal**: Agent 能通过 REST API 完成完整的邮件收发工作流
**Depends on**: Phase 1
**Requirements**: MAIL-01, MAIL-02, MAIL-03, MAIL-04, MAIL-05, MAIL-06
**Success Criteria** (what must be TRUE):
  1. Agent 可通过 API 发送邮件（含 to/cc/bcc、subject、text/html body），邮件成功送达收件人
  2. Agent 可通过 multipart/form-data 发送带附件的邮件
  3. Superadmin key 可指定任意 from 地址发送邮件
  4. Agent 可读取邮件全文、列出会话、搜索消息、回复已有 thread
**Plans**: 2 plans

Plans:
- [ ] 02-01-PLAN.md — Superadmin from 校验放开 + reply_to_thread_id 字段 + 单元测试
- [ ] 02-02-PLAN.md — 读取/列表/搜索端点 agent 场景集成测试

### Phase 3: Webhook Subscriptions
**Goal**: Agent 能订阅邮件事件并通过 webhook 接收实时通知
**Depends on**: Phase 2
**Requirements**: HOOK-01, HOOK-02, HOOK-03, HOOK-04, HOOK-05, HOOK-06
**Success Criteria** (what must be TRUE):
  1. Agent 可创建 webhook 订阅，指定回调 URL 和按联系人/thread 过滤条件
  2. 新邮件到达时，匹配的 webhook 被触发，payload 包含 message ID 和元数据（不含全文）
  3. Webhook payload 使用 HMAC-SHA256 签名，接收方可验证真实性
  4. 投递失败的 webhook 自动以指数退避重试，不因 EventBus lag 丢失事件（DB outbox 模式）
**Plans**: 2 plans

Plans:
- [ ] 03-01-PLAN.md — DB schema + webhook store (CRUD/outbox) + HMAC signer
- [ ] 03-02-PLAN.md — EventBus listener + delivery worker + API routes + server wiring

### Phase 4: MCP Server
**Goal**: Claude Code 等 AI agent 可通过 MCP 协议直接收发邮件
**Depends on**: Phase 2
**Requirements**: MCP-01, MCP-02, MCP-03, MCP-04, MCP-05, MCP-06, MCP-07
**Success Criteria** (what must be TRUE):
  1. MCP server 嵌入 mailrs-server 进程，通过 `/mcp` 端点提供 Streamable HTTP transport
  2. Claude Code 配置 MCP server 后，可通过 send_email / reply_email 工具发送邮件
  3. Claude Code 可通过 read_email / search_emails / list_conversations 工具查阅邮件
  4. MCP 工具使用 API key 认证，权限与 REST API 一致
**Plans**: TBD

Plans:
- [ ] 04-01: TBD
- [ ] 04-02: TBD

## Progress

**Execution Order:**
Phases 1 → 2 → 3 and 4 (parallel). Phase 3 and 4 both depend on Phase 2, but not on each other.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. API Key Authentication | 2/2 | Complete    | 2026-03-09 |
| 2. Agent Email Operations | 2/2 | Complete   | 2026-03-09 |
| 3. Webhook Subscriptions | 0/2 | Planning complete | - |
| 4. MCP Server | 0/? | Not started | - |
