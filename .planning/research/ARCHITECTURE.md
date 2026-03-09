# Architecture Patterns

**Domain:** AI Agent API layer on existing Rust mail server
**Researched:** 2026-03-09 (updated: MCP server in Rust, not TypeScript)

## Recommended Architecture

所有新子系统嵌入现有 mailrs-server 进程。共享同一 `WebState`、PostgreSQL/Valkey 连接池。无独立外部进程。

```
                     Claude Code / AI Agent
                            |
               ┌────────────┼────────────┐
               |            |            |
          MCP (Streamable   REST API    Webhook
           HTTP /mcp)     (/api/...)   (push)
               |            |            |
  ┌────────────┼────────────┼────────────┼────────────┐
  |                    Axum Router                     |
  |                         |                          |
  |  ┌──────────┐  ┌───────┴────────┐  ┌───────────┐  |
  |  | Session  |  | API Key        |  | Rate      |  |
  |  | Auth     |  | Middleware     |  | Limiter   |  |
  |  | (existing)|  | (mlrs_ prefix) |  | (existing)|  |
  |  └──────────┘  └───────┬────────┘  └───────────┘  |
  |                        |                           |
  |            ┌───────────┼───────────┐               |
  |            |           |           |               |
  |  ┌────────┴───┐ ┌─────┴──────┐ ┌──┴─────────┐     |
  |  | /api/mail/* | | /api/agent | | MCP Server |     |
  |  | (existing   | | /keys      | | (rmcp)     |     |
  |  |  + API key  | | /webhooks  | | #[tool]    |     |
  |  |  auth)      | |            | | macros     |     |
  |  └────────────┘ └─────┬──────┘ └──┬─────────┘     |
  |                        |           |               |
  |                  ┌─────┴───────────┴─────┐         |
  |                  | Shared Internal Logic  |         |
  |                  | (MailboxStore, DomainStore,      |
  |                  |  outbound queue, event_bus)      |
  |                  └─────────┬─────────────┘         |
  |                            |                       |
  |  ┌─────────┐ ┌─────────┐  |  ┌─────────┐          |
  |  |PostgreSQL| | Valkey  |  |  | Maildir |          |
  |  |(api_keys,| |(key     |  |  |(message |          |
  |  | webhooks,| | cache)  |  |  | files)  |          |
  |  | deliveries)         |  |  └─────────┘          |
  |  └─────────┘ └─────────┘  |                       |
  └────────────────────────────┼───────────────────────┘
                               |
               Webhook Target Endpoints
               (external consumers)
```

### Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| **API Key Middleware** | Extract `Bearer mlrs_...`，prefix lookup + hash verify，返回 `AuthUser` | Valkey (L1), PostgreSQL (L2) |
| **API Key CRUD** (`/api/agent/keys`) | 创建/列出/撤销 API keys | PostgreSQL `api_keys` 表 |
| **Webhook CRUD** (`/api/agent/webhooks`) | 创建/列出/更新/删除 webhook 订阅 | PostgreSQL `webhooks` 表 |
| **Webhook Delivery Worker** | 订阅 EventBus，匹配过滤，持久化 + 投递 | EventBus, PostgreSQL, reqwest |
| **MCP Server** (rmcp, 嵌入进程) | 声明 MCP tools，通过 Streamable HTTP 暴露 | 直接调用 MailboxStore, DomainStore 等 |
| **Existing Mail API** | 发送/读取/列出邮件 | MailboxStore, outbound queue, Maildir |

### Key Architectural Decision: MCP 嵌入 vs 独立进程

| 方面 | 嵌入 Rust (rmcp) | 独立 TypeScript |
|------|-----------------|----------------|
| 延迟 | 直接函数调用，微秒级 | HTTP roundtrip，毫秒级 |
| 部署 | 单进程，无额外依赖 | 需要 Node.js 运行时 |
| 共享状态 | 直接访问 WebState | 必须通过 REST API |
| 类型安全 | 编译期检查 | 运行时错误 |
| API 漂移 | 不可能，同一代码库 | MCP 工具定义与 API 不同步 |
| 生态成熟度 | rmcp 1.1 已稳定 | TS SDK 更成熟但不需要 |

结论：对于自用场景，嵌入 Rust 优势明显。

### Data Flow

#### API Key Authentication Flow

```
1. Request: Authorization: Bearer mlrs_abc123_XXXXXXXXXXXX
2. API Key Middleware:
   a. 提取 prefix "abc123"（mlrs_ 后 6-8 字符）
   b. Valkey 查找: key = "apikey:{prefix}" -> {key_hash, account_address, expires_at}
   c. Cache miss -> PG: SELECT * FROM api_keys WHERE prefix = $1 AND revoked_at IS NULL
   d. SHA-256(full_token) 与 stored key_hash 比较
   e. 检查 expires_at
   f. 构造 AuthUser(account_address) — 与 session auth 类型相同
3. 请求继续走现有 handler，无需任何改动
```

关键洞察：API key auth 产出与 session auth 相同的 `AuthUser`。所有下游 handler 无需修改。

#### Webhook Event Flow

```
1. SMTP 收到邮件 -> inbound pipeline -> delivery
2. Server emits SmtpEvent::NewMessage { user, thread_id, sender, subject, snippet }
3. Webhook Event Capture Task (订阅 EventBus):
   a. 收到 NewMessage
   b. 查询该 user 的活跃 webhooks
   c. 匹配过滤器 (contact / thread / catch-all)
   d. 为每个匹配的 webhook 插入 webhook_deliveries 记录 (status=pending)
4. Webhook Delivery Worker (独立 tokio task，轮询 pending):
   a. 查询 pending 且 next_retry <= now() 的记录
   b. HTTP POST to webhook.url，带 HMAC-SHA256 签名
   c. 成功: status=delivered
   d. 失败: attempts++，用 backon 计算 next_retry，超过 max_attempts 则 status=dead
```

Event capture 和 delivery 解耦：capture 只做 DB insert（微秒），delivery 做 HTTP POST（可能秒级）。避免 broadcast channel lag 丢事件。

#### MCP Server Flow (Streamable HTTP)

```
1. Claude Code: claude mcp add --transport http --url https://mail.example.com/mcp
2. 连接请求到达 Axum router /mcp 端点
3. rmcp StreamableHttpService 处理 MCP 协议握手
4. Claude Code 调用 tool:
   - send_email(to, subject, body) -> 直接调用 mail::send_message 内部逻辑
   - read_email(uid) -> 直接调用 MailboxStore::get_message
   - list_conversations(folder, limit) -> 直接调用 conversations 查询函数
   - search_emails(query) -> 直接调用搜索逻辑
5. MCP 认证: API key 作为 MCP 连接参数传入，每次 tool 调用时验证
```

## Database Schema Additions

### `api_keys` table

```sql
CREATE TABLE api_keys (
    id BIGSERIAL PRIMARY KEY,
    prefix TEXT NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,              -- SHA-256 of full key
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    scopes TEXT NOT NULL DEFAULT 'all',
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_api_keys_account ON api_keys(account_address) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_prefix ON api_keys(prefix) WHERE revoked_at IS NULL;
```

### `webhooks` table

```sql
CREATE TABLE webhooks (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    url TEXT NOT NULL,
    secret TEXT NOT NULL,
    filter_sender TEXT,
    filter_thread_id TEXT,
    active BOOLEAN NOT NULL DEFAULT true,
    consecutive_failures INT NOT NULL DEFAULT 0,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_webhooks_account ON webhooks(account_address) WHERE active = true;
```

### `webhook_deliveries` table

```sql
CREATE TABLE webhook_deliveries (
    id BIGSERIAL PRIMARY KEY,
    webhook_id BIGINT NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 8,
    next_retry TIMESTAMPTZ,
    last_error TEXT,
    response_status INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ
);
CREATE INDEX idx_wd_pending ON webhook_deliveries(status, next_retry)
    WHERE status = 'pending';
CREATE INDEX idx_wd_webhook ON webhook_deliveries(webhook_id, created_at DESC);
```

## Patterns to Follow

### Pattern 1: Unified Auth Extractor

扩展现有 `AuthUser` extractor，同时处理 session token 和 API key。

```rust
// auth.rs — 扩展现有 FromRequestParts impl
impl FromRequestParts<Arc<WebState>> for AuthUser {
    async fn from_request_parts(parts: &mut Parts, state: &Arc<WebState>) -> Result<Self, Self::Rejection> {
        let token = extract_bearer_token(parts);

        if let Some(token) = token {
            // API key: tokens starting with "mlrs_"
            if token.starts_with("mlrs_") {
                return verify_api_key(&token, state).await;
            }
            // session token (existing logic)
            if let Some(session) = state.sessions.get(token.as_str()) {
                if session.created_at.elapsed() < SESSION_TTL {
                    return Ok(AuthUser(session.address.clone()));
                }
            }
        }
        Err((StatusCode::UNAUTHORIZED, "authentication required"))
    }
}
```

### Pattern 2: Event Capture + Async Delivery (Outbox Pattern)

EventBus subscriber 只做 DB 写入，独立 worker 做 HTTP 投递。

```rust
// event capture: fast, never blocks
pub fn spawn_webhook_capture(state: Arc<WebState>) {
    tokio::spawn(async move {
        let mut rx = state.event_bus.subscribe();
        while let Ok(event) = rx.recv().await {
            if let SmtpEvent::NewMessage { ref user, .. } = event {
                // insert into webhook_deliveries (microseconds)
                insert_pending_deliveries(&state, user, &event).await;
            }
        }
    });
}

// delivery worker: slow, retries
pub fn spawn_webhook_delivery(state: Arc<WebState>) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        loop {
            // poll pending deliveries where next_retry <= now()
            deliver_pending(&state, &client).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
```

### Pattern 3: MCP Tools via rmcp Macros

```rust
use rmcp::prelude::*;

#[derive(Clone)]
struct MailrsMcp {
    state: Arc<WebState>,
    user: String,
}

#[tool(tool_box)]
impl MailrsMcp {
    #[tool(description = "Send an email")]
    async fn send_email(
        &self,
        #[tool(param, description = "recipient email addresses")] to: Vec<String>,
        #[tool(param, description = "email subject")] subject: String,
        #[tool(param, description = "email body (plain text)")] body: String,
    ) -> String {
        // directly call internal send logic with self.state
        // no HTTP roundtrip needed
        "Email sent successfully".to_string()
    }

    #[tool(description = "Read an email by UID")]
    async fn read_email(
        &self,
        #[tool(param, description = "message UID")] uid: i64,
    ) -> String {
        // directly call MailboxStore::get_message
        "...email content...".to_string()
    }
}
```

### Pattern 4: Webhook HMAC Signing

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

fn sign_payload(secret: &str, payload: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}
// Header: X-Mailrs-Signature: sha256={hex}
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Separate API Routes for Agent
**What:** 创建 `/api/agent/mail/send` 复制 `/api/mail/send` 逻辑
**Instead:** 统一 auth extractor，现有路由自动支持 API key

### Anti-Pattern 2: Webhook Delivery in Request Path
**What:** 同步投递 webhook
**Instead:** EventBus -> DB insert (fast) -> async delivery worker (slow)

### Anti-Pattern 3: Storing Raw API Keys
**What:** 明文存 API key
**Instead:** SHA-256 hash + prefix，创建时返回一次

### Anti-Pattern 4: MCP Server with Direct DB Access (Not Applicable)
原设计中 TypeScript MCP server 不应直连 DB。现在用 Rust 嵌入，MCP tools 通过 WebState 间接访问 DB，走标准的 MailboxStore/DomainStore 抽象层，不直接写 SQL。

### Anti-Pattern 5: Broadcast Channel Direct Delivery
**What:** 在 broadcast::recv() 循环中做 HTTP POST
**Instead:** capture 写 DB，delivery worker 独立轮询

## Integration Points with Existing Code

| Existing Component | How New Code Integrates |
|---|---|
| `WebState` | 新增 `api_key_cache: Option<ApiKeyCache>` 字段 |
| `AuthUser` extractor | 扩展 `from_request_parts` 处理 `mlrs_` prefix |
| `EventBus` | Webhook capture task 调用 `subscribe()` |
| `SmtpEvent::NewMessage` | 已含 user, thread_id, sender — webhook 过滤所需 |
| `router()` | 新增 `.nest("/api/agent", agent_routes)` + MCP 路由 |
| `spawn_session_cleanup()` | 并行启动 `spawn_webhook_capture()` + `spawn_webhook_delivery()` |
| PostgreSQL pool | 复用 `state.pg_pool` |
| Valkey connection | 复用 `state.valkey` 做 API key cache |
| Rate limiter | 现有 `WebRateLimiter` 自动应用 |

## Sources

- [rmcp on crates.io](https://crates.io/crates/rmcp) — v1.1.0
- [Shuttle: Streamable HTTP MCP Server in Rust](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp)
- [MCP TypeScript SDK](https://github.com/modelcontextprotocol/typescript-sdk) — 参考但不使用
- [Webhook Delivery Patterns](https://hookdeck.com/webhooks/guides/webhook-retry-best-practices)
- [API Key Best Practices](https://articles.mergify.com/api-keys-best-practice/)
- mailrs codebase: auth.rs, event_bus.rs, web/mod.rs
