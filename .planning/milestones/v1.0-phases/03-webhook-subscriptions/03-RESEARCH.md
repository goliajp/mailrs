# Phase 3: Webhook Subscriptions - Research

**Researched:** 2026-03-10
**Domain:** Webhook subscription system (DB outbox pattern, async HTTP delivery, HMAC signing)
**Confidence:** HIGH

## Summary

Phase 3 在 mailrs 现有基础设施上构建 webhook 订阅系统。核心模式已有成熟先例：`outbound_queue` crate 实现了完整的 DB 持久化 + 轮询 + 指数退避重试 + Valkey 通知加速的工作模式，webhook 投递 worker 可以复用相同架构。`EventBus` 的 `SmtpEvent::NewMessage` 事件已包含 `user`、`thread_id`、`sender`、`subject`、`snippet`，恰好覆盖 HOOK-02 和 HOOK-03 的过滤需求以及 HOOK-04 的 metadata payload。

关键设计决策（已在 STATE.md 中锁定）：**使用 DB outbox 模式**，不直接依赖 EventBus broadcast。这意味着需要一个 listener 监听 EventBus 事件、匹配订阅、写入 outbox 表，再由独立 worker 异步投递。HMAC-SHA256 签名使用 `hmac` + `sha2` crate（`sha2` 已在依赖中）。

**Primary recommendation:** 在 server crate 内新增 `webhook` 模块（非独立 crate），包含 store/worker/signer 三个子模块，复用 outbound_queue 的 poll-deliver-retry 架构模式。

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| HOOK-01 | Agent can create webhook subscription (URL + event type) | PG `webhook_subscriptions` 表 + CRUD API endpoints under `/api/agent/webhooks` |
| HOOK-02 | Webhook can filter by contact email address | `SmtpEvent::NewMessage.sender` 字段用于匹配 `filter_sender` 列 |
| HOOK-03 | Webhook can filter by thread ID | `SmtpEvent::NewMessage.thread_id` 字段用于匹配 `filter_thread_id` 列 |
| HOOK-04 | Webhook payload contains only message ID + metadata (not full content) | Payload 包含 event_type, message_id, thread_id, sender, subject, timestamp；不含 body/html |
| HOOK-05 | Failed webhook deliveries retry with exponential backoff | DB outbox 表 + worker 轮询 + `retry_delay_secs()` 风格的退避逻辑 |
| HOOK-06 | Webhook payload signed with HMAC-SHA256 | `hmac` crate + per-subscription `signing_secret`，签名放入 `X-Mailrs-Signature` header |
</phase_requirements>

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| sqlx | 0.8 | Webhook 表 CRUD + outbox 队列 | 项目已用，runtime query 模式 |
| reqwest | 0.12 | HTTP POST 投递 webhook payload | 项目已有依赖，rustls-tls |
| serde/serde_json | 1 | Payload JSON 序列化 | 项目已用 |
| sha2 | 0.10 | HMAC-SHA256 的 hash 部分 | 项目已有依赖 |
| tokio | 1 | Async worker runtime | 项目已用 |
| chrono | 0.4 | Timestamp 处理 | 项目已用 |
| redis | 0.27 | Valkey pubsub 快速唤醒 worker | 项目已用 |

### New Dependencies
| Library | Version | Purpose | Why Needed |
|---------|---------|---------|------------|
| hmac | 0.12 | HMAC-SHA256 签名计算 | 标准 Rust HMAC 实现，配合已有 sha2 |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| hmac crate | ring | ring 功能更多但更重，hmac + sha2 已够 |
| reqwest | hyper 直连 | reqwest 更简单，已在依赖中 |
| DB outbox | 直接从 EventBus 投递 | EventBus broadcast channel 有 lag 和容量限制，丢事件不可恢复 |

**Installation:**
```toml
# add to crates/server/Cargo.toml
hmac = "0.12"
```

## Architecture Patterns

### Recommended Module Structure
```
crates/server/src/
├── webhook/
│   ├── mod.rs           # pub exports, types
│   ├── store.rs         # PG CRUD for subscriptions + outbox
│   ├── worker.rs        # background delivery worker (poll + deliver + retry)
│   ├── signer.rs        # HMAC-SHA256 signing logic
│   └── listener.rs      # EventBus listener → match subscriptions → write outbox
├── web/
│   └── webhook.rs       # Axum route handlers (CRUD endpoints)
```

### Pattern 1: DB Outbox (from outbound_queue)
**What:** 事件先持久化到 PG outbox 表，独立 worker 轮询投递
**When to use:** 任何不能丢失的异步外发操作
**Why:** EventBus 是 Tokio broadcast channel，subscriber 落后会 Lagged 丢消息。DB outbox 保证 at-least-once delivery。

已有参考实现：
```rust
// outbound_queue/src/worker.rs 的 poll-deliver 循环
loop {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(poll_interval)) => {}
        _ = wait_for_notify(&mut notify_rx) => {}  // Valkey pubsub 快速唤醒
        _ = shutdown.changed() => { return; }
    }
    self.poll_and_deliver().await;
}
```

Webhook worker 应复用相同模式：poll → dequeue pending → POST to URL → mark delivered/failed。

### Pattern 2: EventBus Listener → Outbox Writer
**What:** 一个持久运行的 task 订阅 EventBus，匹配 webhook 订阅条件，写入 outbox
**When to use:** 将 in-memory 事件转换为持久化队列记录

```rust
// webhook/listener.rs
pub async fn run(event_bus: &EventBus, pool: &PgPool, mut shutdown: watch::Receiver<bool>) {
    let mut rx = event_bus.subscribe();
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(SmtpEvent::NewMessage { user, thread_id, sender, subject, snippet }) => {
                        // query matching subscriptions
                        // for each match: insert into webhook_outbox
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("webhook listener lagged, missed {n} events");
                        // lagged events are lost — this is acceptable because
                        // the outbox pattern means we only lose events that
                        // weren't yet written, and the listener should keep up
                    }
                    _ => {}
                }
            }
            _ = shutdown.changed() => { return; }
        }
    }
}
```

### Pattern 3: HMAC-SHA256 Signing
**What:** 每个 webhook 订阅有独立的 signing_secret，payload 签名放入 HTTP header
**When to use:** HOOK-06 requirement

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn sign_payload(secret: &[u8], payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key size");
    mac.update(payload);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}
// Header: X-Mailrs-Signature: sha256=<hex>
```

### Anti-Patterns to Avoid
- **直接从 EventBus 发 HTTP 请求:** EventBus handler 内做 I/O 会阻塞事件分发，且丢 lag 事件不可恢复
- **Webhook secret 明文存储:** signing_secret 应该只在创建时返回一次（类似 API key），数据库存哈希。但 webhook 签名需要原始 secret 来计算 HMAC——所以 **signing_secret 必须可逆存储**（加密或明文），不能哈希。这与 API key 不同
- **无限重试:** 必须设 max_attempts，超过后标记为 permanently_failed
- **同步投递阻塞主循环:** 投递应并发，用 semaphore 限制并发数

## Database Schema

### webhook_subscriptions 表
```sql
CREATE TABLE webhook_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    url TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'new_message',
    filter_sender TEXT,          -- NULL = match all senders
    filter_thread_id TEXT,       -- NULL = match all threads
    signing_secret TEXT NOT NULL, -- plaintext, needed for HMAC computation
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_webhook_subs_account ON webhook_subscriptions(account_address) WHERE active = true;
CREATE INDEX idx_webhook_subs_event ON webhook_subscriptions(event_type, active) WHERE active = true;
```

### webhook_outbox 表
```sql
CREATE TABLE webhook_outbox (
    id BIGSERIAL PRIMARY KEY,
    subscription_id BIGINT NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, inflight, delivered, failed
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    next_retry BIGINT NOT NULL,
    last_error TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX idx_webhook_outbox_pending ON webhook_outbox(status, next_retry)
    WHERE status = 'pending';
```

### API Endpoints
```
POST   /api/agent/webhooks          — 创建 webhook 订阅
GET    /api/agent/webhooks          — 列出当前用户的 webhook 订阅
DELETE /api/agent/webhooks/{id}     — 删除 webhook 订阅
```

### Webhook Payload (HOOK-04)
```json
{
    "event": "new_message",
    "timestamp": "2026-03-10T12:00:00Z",
    "data": {
        "user": "user@golia.jp",
        "thread_id": "abc123",
        "sender": "someone@example.com",
        "subject": "Hello",
        "snippet": "First 100 chars of text..."
    }
}
```

Headers:
```
Content-Type: application/json
X-Mailrs-Signature: sha256=<hex hmac>
X-Mailrs-Event: new_message
X-Mailrs-Delivery: <outbox_id>
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HMAC-SHA256 | Manual hash construction | `hmac` + `sha2` crates | Timing-safe comparison, correct padding |
| HTTP client | Raw TCP/TLS | `reqwest` | 连接池、重定向、超时、TLS 都已处理 |
| Exponential backoff delays | Custom math | 复用 outbound_queue 的 `retry_delay_secs` 模式 | 已验证的延迟表 |
| Secret generation | Custom random | `rand_core::OsRng` + `hex::encode` | 与 api_key_store 一致 |

## Common Pitfalls

### Pitfall 1: EventBus Lagged 丢事件
**What goes wrong:** broadcast channel 容量溢出，subscriber 收到 Lagged error
**Why it happens:** webhook listener 处理速度跟不上事件发射速度（如大量并发邮件到达）
**How to avoid:** listener 只做 match + INSERT outbox（轻量 PG 写），不做 HTTP 投递。Lagged 事件记 warn 日志
**Warning signs:** 日志出现 "webhook listener lagged"

### Pitfall 2: Webhook URL SSRF
**What goes wrong:** Agent 设置内网 URL（如 http://169.254.169.254）作为 webhook 回调
**Why it happens:** 未校验 URL 目标
**How to avoid:** 验证 URL scheme（仅 https，开发环境允许 http）；可选：DNS 解析后拒绝私有 IP 段
**Warning signs:** webhook 投递到非公网地址

### Pitfall 3: Signing Secret 存储混淆
**What goes wrong:** 像 API key 一样对 signing_secret 做 SHA-256 哈希存储，导致无法计算 HMAC
**Why it happens:** API key 模式是"验证 hash 相等"，webhook 签名需要原始 secret
**How to avoid:** signing_secret 在 DB 中明文存储（或用 server-side encryption），创建时返回给用户

### Pitfall 4: 投递超时过长阻塞 Worker
**What goes wrong:** 某个 webhook URL 响应极慢，worker 线程被阻塞
**Why it happens:** reqwest 默认无超时
**How to avoid:** 设置 reqwest timeout（10 秒），connect_timeout（5 秒）。用 semaphore 限并发

### Pitfall 5: 订阅查询性能
**What goes wrong:** 每个 NewMessage 事件都全表扫描 webhook_subscriptions
**Why it happens:** 订阅数量增长后查询变慢
**How to avoid:** 按 account_address + active 索引查询；可选：内存缓存（DashMap）订阅列表，subscription 变更时 invalidate

## Code Examples

### Webhook Store (CRUD)
```rust
// webhook/store.rs — insert subscription
pub async fn create_subscription(
    pool: &PgPool,
    account: &str,
    url: &str,
    event_type: &str,
    filter_sender: Option<&str>,
    filter_thread_id: Option<&str>,
    signing_secret: &str,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_subscriptions (account_address, url, event_type, filter_sender, filter_thread_id, signing_secret)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"
    )
    .bind(account).bind(url).bind(event_type)
    .bind(filter_sender).bind(filter_thread_id).bind(signing_secret)
    .fetch_one(pool).await?;
    Ok(row.0)
}
```

### Outbox Enqueue (from listener)
```rust
// webhook/store.rs — write outbox entry
pub async fn enqueue_delivery(
    pool: &PgPool,
    subscription_id: i64,
    payload: &serde_json::Value,
    now: i64,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_outbox (subscription_id, payload, status, next_retry, created_at, updated_at)
         VALUES ($1, $2, 'pending', $3, $3, $3) RETURNING id"
    )
    .bind(subscription_id).bind(payload).bind(now)
    .fetch_one(pool).await?;
    Ok(row.0)
}
```

### Delivery Worker (simplified)
```rust
// webhook/worker.rs
async fn deliver_one(client: &reqwest::Client, sub: &Subscription, outbox: &OutboxEntry) -> Result<(), String> {
    let payload_bytes = serde_json::to_vec(&outbox.payload).map_err(|e| e.to_string())?;
    let signature = sign_payload(sub.signing_secret.as_bytes(), &payload_bytes);

    let resp = client.post(&sub.url)
        .header("Content-Type", "application/json")
        .header("X-Mailrs-Signature", format!("sha256={signature}"))
        .header("X-Mailrs-Event", &outbox.event_type)
        .body(payload_bytes)
        .timeout(Duration::from_secs(10))
        .send().await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| EventBus 直投 | DB outbox + async worker | 架构决策 (STATE.md) | 保证 at-least-once，不丢事件 |
| 共享 signing key | Per-subscription signing secret | 行业标准 | 单个订阅泄露不影响其他 |

## Open Questions

1. **Webhook URL validation 严格程度**
   - What we know: 需要防 SSRF，至少校验 scheme
   - What's unclear: 是否需要 DNS 解析后检查私有 IP（增加复杂度）
   - Recommendation: v1 仅校验 https scheme（dev 允许 http），不做 IP 检查；v2 考虑加强

2. **Subscription 内存缓存**
   - What we know: 每个 NewMessage 需要查匹配的 subscriptions
   - What's unclear: 订阅数量级（目前自用场景可能 < 10）
   - Recommendation: v1 直接查 PG（有索引），足够快；v2 如需优化加 DashMap 缓存

3. **Listener Lagged 恢复**
   - What we know: broadcast channel lagged 意味着部分事件丢失
   - What's unclear: 是否需要补偿机制（如定期扫描近期 messages 表）
   - Recommendation: v1 接受 lagged 丢失（自用场景事件量低），记 warn 日志；v2 考虑 WAL-based 方案

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml workspace test config |
| Quick run command | `cargo test -p mailrs-server -- webhook` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HOOK-01 | Create/list/delete webhook subscription | unit | `cargo test -p mailrs-server -- webhook::store::tests -x` | Wave 0 |
| HOOK-02 | Filter by sender email | unit | `cargo test -p mailrs-server -- webhook::listener::tests::filter_sender -x` | Wave 0 |
| HOOK-03 | Filter by thread ID | unit | `cargo test -p mailrs-server -- webhook::listener::tests::filter_thread -x` | Wave 0 |
| HOOK-04 | Payload contains metadata only | unit | `cargo test -p mailrs-server -- webhook::tests::payload_format -x` | Wave 0 |
| HOOK-05 | Retry with exponential backoff | unit | `cargo test -p mailrs-server -- webhook::worker::tests -x` | Wave 0 |
| HOOK-06 | HMAC-SHA256 signing | unit | `cargo test -p mailrs-server -- webhook::signer::tests -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p mailrs-server -- webhook`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/server/src/webhook/mod.rs` — module definition
- [ ] `crates/server/src/webhook/store.rs` — subscription + outbox CRUD with tests
- [ ] `crates/server/src/webhook/signer.rs` — HMAC signing with tests
- [ ] `crates/server/src/webhook/listener.rs` — event matching logic with tests
- [ ] `crates/server/src/webhook/worker.rs` — delivery + retry with tests
- [ ] `crates/server/src/web/webhook.rs` — API route handlers
- [ ] Schema migration: `webhook_subscriptions` + `webhook_outbox` tables

## Sources

### Primary (HIGH confidence)
- Codebase: `crates/outbound-queue/src/` — DB outbox + retry pattern reference
- Codebase: `crates/server/src/event_bus.rs` — SmtpEvent::NewMessage structure
- Codebase: `crates/server/src/web/api_key.rs` — CRUD endpoint pattern
- Codebase: `crates/server/src/api_key_store.rs` — secret generation pattern
- Codebase: `scripts/init-schema.sql` — existing table conventions
- Codebase: `crates/server/Cargo.toml` — existing dependencies (sha2, reqwest, hex)

### Secondary (MEDIUM confidence)
- `hmac` crate 0.12 API — standard RustCrypto HMAC interface, well-established

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - 所有核心依赖已在项目中，仅新增 hmac
- Architecture: HIGH - outbox pattern 在 outbound_queue 中已有完整实现，直接复用
- Pitfalls: HIGH - 基于对现有 EventBus 和 outbound_queue 代码的直接分析

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, no fast-moving dependencies)
