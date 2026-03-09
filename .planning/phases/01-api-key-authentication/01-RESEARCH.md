# Phase 1: API Key Authentication - Research

**Researched:** 2026-03-10
**Domain:** API key CRUD, hashed storage, Bearer authentication, role inheritance
**Confidence:** HIGH

## Summary

Phase 1 为 mailrs 添加 API key 认证系统，让 AI agent 能通过 `Authorization: Bearer mlrs_...` 访问所有现有 API。核心设计原则是：API key 认证产出与 session 认证相同的 `AuthUser` 类型，所有下游 handler 零修改。

现有代码库已具备所有必需依赖（sha2 0.10, hex 0.4, rand_core 0.6, argon2 0.5, sqlx 0.8, redis 0.27, dashmap 6）。新增代码量约 400-600 行 Rust，集中在 `auth.rs` 扩展、新的 `api_key.rs` 模块、和 SQL migration。

**Primary recommendation:** 扩展现有 `AuthUser::from_request_parts` 识别 `mlrs_` prefix token，新建 `api_keys` PG 表 + Valkey 缓存层，CRUD 路由挂在 `/api/agent/keys`。

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AKEY-01 | User can create API key for their account | CRUD 路由 + `rand_core::OsRng` 生成 32 bytes random，拼 `mlrs_` prefix |
| AKEY-02 | API key shown once on creation, stored as SHA-256 hash | `sha2::Sha256` 对 full key hash 后存 PG，响应只返回一次明文 |
| AKEY-03 | API key uses `mlrs_` prefix, first 8 chars stored as plaintext identifier | prefix = `mlrs_` + 8 hex chars（从 random bytes 前 4 bytes 取），用作 DB lookup key |
| AKEY-04 | API key authenticates via `Authorization: Bearer <key>` | 扩展 `AuthUser::from_request_parts`，`token.starts_with("mlrs_")` 分支 |
| AKEY-05 | User can revoke API key with immediate effect (including Valkey cache eviction) | revoke endpoint 同时写 PG `revoked_at` + 删 Valkey `apikey:{prefix}` |
| AKEY-06 | API key inherits account role; superadmin key can operate any mailbox | `AuthUser` 已携带 account address，session 的 `super_domains` 查找需适配 API key 场景 |
| AKEY-07 | API key supports optional expiration time | `expires_at TIMESTAMPTZ` 列，认证时检查 `expires_at IS NULL OR expires_at > now()` |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| sha2 | 0.10 | API key hash (SHA-256) | 已在 server Cargo.toml，用于 key 的快速 hash 存储 |
| hex | 0.4 | Hash 编码 | 已在 server Cargo.toml |
| rand_core | 0.6 | Key 随机生成 | 已在 server Cargo.toml，`OsRng` CSPRNG |
| argon2 | 0.5 | (备选) Key hash | 已在 server Cargo.toml，但 SHA-256 足够 — 见下方分析 |
| sqlx | 0.8 | `api_keys` 表操作 | 已在 workspace dependencies |
| redis | 0.27 | API key Valkey 缓存 | 已在 workspace dependencies |
| dashmap | 6 | 进程内缓存 | 已在 server Cargo.toml |
| axum | 0.8 | HTTP 路由 + extractor | 已在 server Cargo.toml |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| SHA-256 for key hash | Argon2 | Argon2 更抗暴力破解，但 API key 本身有 256-bit entropy，SHA-256 足够。Argon2 每次验证 ~100ms，不适合高频 API 调用 |
| Valkey L1 cache | 纯 PG 查询 | 每次 API 调用都查 PG 太慢。Valkey 缓存 + short TTL 是标准模式 |
| `mlrs_` prefix 8 char | UUID prefix | 8 hex chars = 4 bytes = 32-bit，足够作为 lookup key，碰撞概率极低 |

### Hash Strategy Decision: SHA-256 (not Argon2)

REQUIREMENTS.md 明确写 "stored as SHA-256 hash"。这是正确选择：
1. API key 有 256-bit entropy（32 random bytes），暴力破解不可行
2. SHA-256 验证 ~1 microsecond vs Argon2 ~100ms
3. API 可能每秒收到多次调用，Argon2 会成为瓶颈
4. 密码需要 Argon2 因为 entropy 低（人类选择的密码），API key 不存在这个问题

**Confidence: HIGH** — 这是业界标准做法（Stripe, GitHub, OpenAI 都用 SHA-256/HMAC 存 API key）。

## Architecture Patterns

### Recommended Project Structure
```
crates/server/src/
├── web/
│   ├── auth.rs          # 扩展 AuthUser extractor 支持 mlrs_ prefix
│   ├── api_key.rs       # NEW: API key CRUD handlers + types
│   └── mod.rs           # 新增 /api/agent/keys 路由
├── api_key_store.rs     # NEW: API key DB/cache 操作
└── ...
scripts/
├── migrate-XXX-api-keys.sql  # NEW: api_keys 表
```

### Pattern 1: Unified Auth Extractor (扩展 AuthUser)
**What:** 在现有 `AuthUser::from_request_parts` 中添加 `mlrs_` 分支
**When to use:** 所有需要认证的 API 调用
**Example:**
```rust
// auth.rs — 扩展现有 FromRequestParts impl
impl FromRequestParts<Arc<WebState>> for AuthUser {
    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<WebState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers.get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
            Some(t.to_string())
        } else {
            // fallback: ?token= query param (existing, for session tokens only)
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .find_map(|pair| pair.strip_prefix("token="))
                    .map(|t| t.to_string())
            })
        };

        if let Some(ref token) = token {
            // API key path: mlrs_ prefix
            if token.starts_with("mlrs_") {
                return verify_api_key(token, state).await;
            }
            // session token path (existing logic)
            if let Some(session) = state.sessions.get(token.as_str()) {
                if session.created_at.elapsed() < super::SESSION_TTL {
                    return Ok(AuthUser(session.address.clone()));
                }
                drop(session);
                state.sessions.remove(token.as_str());
            }
        }
        Err((StatusCode::UNAUTHORIZED, "authentication required"))
    }
}
```

### Pattern 2: API Key Format
**What:** `mlrs_` + 8 hex chars (prefix) + `_` + 40 hex chars (secret)
**Total length:** 5 + 8 + 1 + 40 = 54 characters
**Example:** `mlrs_a1b2c3d4_e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8`

```rust
fn generate_api_key() -> (String, String) {
    let mut prefix_bytes = [0u8; 4];
    let mut secret_bytes = [0u8; 20];
    OsRng.fill_bytes(&mut prefix_bytes);
    OsRng.fill_bytes(&mut secret_bytes);

    let prefix = hex::encode(prefix_bytes); // 8 hex chars
    let secret = hex::encode(secret_bytes); // 40 hex chars
    let full_key = format!("mlrs_{prefix}_{secret}");
    let key_hash = sha256_hex(full_key.as_bytes());

    (full_key, key_hash)
    // prefix stored in DB for lookup
    // key_hash stored in DB for verification
    // full_key returned to user ONCE
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(data);
    hex::encode(hash)
}
```

### Pattern 3: Three-Layer Cache (同 DomainStore 模式)
**What:** Valkey L1 -> PostgreSQL L2 -> (no L3 for API keys, security reasons)
**When to use:** API key 验证
```
1. Extract prefix from token: "mlrs_{prefix}_..." -> prefix
2. Valkey GET "apikey:{prefix}" -> CachedApiKey { key_hash, account_address, expires_at }
3. Cache miss -> PG: SELECT ... FROM api_keys WHERE prefix = $1 AND revoked_at IS NULL
4. SHA-256(full_token) == stored key_hash?
5. Check expires_at
6. Return AuthUser(account_address)
7. Async: update last_used_at
```

注意：不做进程内 DashMap L3 缓存。API key 需要能被立即撤销，进程内缓存无法跨实例失效。Valkey 是唯一缓存层。

### Pattern 4: SessionInfo for API Key Auth
**What:** API key 认证后需要 `super_domains` 信息用于权限检查
**Problem:** 现有 `validate_domains()` 从 `state.sessions` DashMap 查找 `super_domains`。API key 认证不经过 login，sessions 中没有记录。
**Solution:** 两个选择：

**Option A (推荐):** 把 `super_domains` 等信息放入 `AuthUser` struct，而非从 sessions 查找
```rust
pub(crate) struct AuthUser {
    pub address: String,
    pub display_name: String,
    pub super_domains: Vec<String>,
    pub auth_method: AuthMethod, // Session | ApiKey(i64)
}

pub(crate) enum AuthMethod {
    Session,
    ApiKey(i64), // api_key id for audit
}
```
这需要修改所有 `AuthUser(user)` 解构为 `AuthUser { address, .. }` 模式。影响范围可控（grep 显示 ~20 处使用）。

**Option B:** API key 认证时向 sessions DashMap 注入一个临时 session。较 hacky，不推荐。

### Pattern 5: API Key CRUD Routes
```
POST   /api/agent/keys          # create key (requires session auth)
GET    /api/agent/keys          # list keys (metadata only, no full key)
DELETE /api/agent/keys/{id}     # revoke key
```

创建 API key 必须用 session auth（用户登录后在 web UI 创建）。API key 本身也可以创建新 key（需评估安全性，v1 先只允许 session）。

### Anti-Patterns to Avoid
- **API key in query params:** `mlrs_` token 只通过 `Authorization: Bearer` header 接受。`?token=` 路径保留给 session token（用于 `<img src>` 等场景）
- **Storing raw API keys:** 绝不存明文。DB 只存 SHA-256 hash
- **Argon2 for API key verification:** 太慢，不适合高频调用
- **Process-level API key cache (DashMap):** 无法跨实例立即失效，安全风险
- **Separate API routes for agent:** 不创建 `/api/agent/mail/send`，统一 auth extractor 让现有路由自动支持 API key

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Random key generation | 自己实现 random | `rand_core::OsRng.fill_bytes()` | CSPRNG，已在项目中 |
| SHA-256 hashing | 手写 hash | `sha2::Sha256` + `hex::encode` | 标准实现，已在项目中 |
| Cache invalidation | 自定义 pub/sub | Valkey DEL + short TTL | 已有 Valkey 连接，直接用 |
| Rate limiting | 手写限流 | 现有 `WebRateLimiter` | API key 请求自动经过现有 rate limit middleware |

## Common Pitfalls

### Pitfall 1: Revoked Key Cache Staleness
**What goes wrong:** 撤销 API key 后 Valkey 缓存仍有效
**Why it happens:** 只更新 PG 的 `revoked_at`，忘了删 Valkey 条目
**How to avoid:** revoke handler 必须同时：(1) UPDATE api_keys SET revoked_at = now() (2) Valkey DEL "apikey:{prefix}"
**Warning signs:** 没有 "revoke -> immediate 401" 的集成测试

### Pitfall 2: super_domains Lookup Fails for API Key Auth
**What goes wrong:** API key 认证后，`validate_domains()` 和 `auth_me` 从 `state.sessions` 查找 `super_domains`，找不到（API key 不在 sessions 中）
**Why it happens:** 现有代码假设所有认证用户都有 session 记录
**How to avoid:** 重构 `AuthUser` 携带 `super_domains` 信息，或在 `verify_api_key` 时查询 account 并注入 session
**Warning signs:** superadmin API key 无法访问跨域资源

### Pitfall 3: API Key Prefix Collision
**What goes wrong:** 两个 key 有相同的 8-char prefix
**Why it happens:** 8 hex chars = 32 bits，理论上 ~65K keys 后碰撞概率显著（birthday paradox）
**How to avoid:** DB 有 UNIQUE constraint on prefix。创建时如果碰撞，重新生成
**Warning signs:** 无 UNIQUE constraint 或无碰撞重试逻辑

### Pitfall 4: last_used_at Update Blocking Request
**What goes wrong:** 每次 API 调用同步更新 `last_used_at`，增加延迟
**How to avoid:** 异步更新（tokio::spawn）或批量更新（每 N 分钟聚合一次）
**Warning signs:** API key 验证路径中有 `UPDATE ... SET last_used_at = now()` 的 `.await`

### Pitfall 5: Migration 遗漏 accounts.super_domains 列
**What goes wrong:** `api_keys` migration 正常，但忘了 `super_domains` 已是通过 migrate-002 添加的，不在 init-schema.sql 中
**How to avoid:** 确认 `accounts` 表有 `super_domains` 列（已通过 migrate-002-supermode.sql 确认存在）

## Code Examples

### Database Schema: api_keys table

```sql
-- scripts/migrate-XXX-api-keys.sql
CREATE TABLE api_keys (
    id          BIGSERIAL PRIMARY KEY,
    prefix      TEXT NOT NULL UNIQUE,           -- first 8 hex chars after mlrs_
    key_hash    TEXT NOT NULL,                   -- SHA-256 of full key
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',        -- user-friendly label
    expires_at  TIMESTAMPTZ,                    -- NULL = never expires
    last_used_at TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ,                    -- NULL = active
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- partial index: only active keys
CREATE INDEX idx_api_keys_prefix_active ON api_keys(prefix) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_account ON api_keys(account_address) WHERE revoked_at IS NULL;
```

### Valkey Cache Structure

```
Key:   "apikey:{prefix}"        -- e.g. "apikey:a1b2c3d4"
Value: JSON { key_hash, account_address, super_domains, expires_at }
TTL:   300 seconds (5 minutes, same as account cache)
```

### API Key Verification Flow

```rust
async fn verify_api_key(
    token: &str,
    state: &Arc<WebState>,
) -> Result<AuthUser, (StatusCode, &'static str)> {
    // parse: "mlrs_{prefix}_{secret}"
    let parts: Vec<&str> = token.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0] != "mlrs" {
        return Err((StatusCode::UNAUTHORIZED, "invalid api key format"));
    }
    let prefix = parts[1];
    let cache_key = format!("apikey:{prefix}");

    // L1: Valkey cache
    let cached = valkey_get_api_key(state, &cache_key).await;

    // L2: PostgreSQL
    let key_record = match cached {
        Some(record) => record,
        None => {
            let record = pg_get_api_key(state, prefix).await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "auth backend error"))?
                .ok_or((StatusCode::UNAUTHORIZED, "invalid api key"))?;
            // backfill cache
            valkey_set_api_key(state, &cache_key, &record).await;
            record
        }
    };

    // verify hash
    let token_hash = sha256_hex(token.as_bytes());
    if token_hash != key_record.key_hash {
        return Err((StatusCode::UNAUTHORIZED, "invalid api key"));
    }

    // check expiration
    if let Some(expires_at) = key_record.expires_at {
        if expires_at < chrono::Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "api key expired"));
        }
    }

    // async update last_used_at (fire and forget)
    let state2 = state.clone();
    let key_id = key_record.id;
    tokio::spawn(async move {
        update_last_used(&state2, key_id).await;
    });

    Ok(AuthUser {
        address: key_record.account_address,
        // ... super_domains from account lookup
    })
}
```

### CRUD Handler: Create API Key

```rust
// POST /api/agent/keys
async fn create_api_key(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,  // must be session-authenticated
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    let (full_key, prefix, key_hash) = generate_api_key();

    // insert into PG
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO api_keys (prefix, key_hash, account_address, name, expires_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id"
    )
    .bind(&prefix)
    .bind(&key_hash)
    .bind(&address)
    .bind(&req.name)
    .bind(&req.expires_at)
    .fetch_one(pool)
    .await?;

    Json(json!({
        "id": id,
        "key": full_key,  // SHOWN ONCE
        "prefix": prefix,
        "name": req.name,
        "warning": "Save this key now. It cannot be retrieved again."
    }))
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Argon2/bcrypt for API keys | SHA-256 for high-entropy tokens | 一直如此 | SHA-256 足够，性能好 100x+ |
| API key as UUID | Prefixed keys (sk_, pk_, mlrs_) | ~2020 Stripe popularized | 便于识别来源，前缀可做 fast lookup |
| 全量 key hash comparison | Prefix lookup + hash verify | 标准做法 | O(1) lookup instead of O(n) |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) + vitest (frontend) |
| Config file | None (Cargo.toml) |
| Quick run command | `cargo test -p mailrs-server` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AKEY-01 | Create API key returns full key + metadata | unit | `cargo test -p mailrs-server api_key::tests::create_key -- --exact` | Wave 0 |
| AKEY-02 | Key stored as SHA-256, never recoverable | unit | `cargo test -p mailrs-server api_key::tests::key_hash_is_sha256 -- --exact` | Wave 0 |
| AKEY-03 | Key format: mlrs_ prefix + 8 char identifier | unit | `cargo test -p mailrs-server api_key::tests::key_format_valid -- --exact` | Wave 0 |
| AKEY-04 | Bearer auth with API key returns AuthUser | integration | `cargo test -p mailrs-server api_key::tests::bearer_auth_works -- --exact` | Wave 0 |
| AKEY-05 | Revoked key returns 401 + Valkey evicted | integration | `cargo test -p mailrs-server api_key::tests::revoke_immediate_effect -- --exact` | Wave 0 |
| AKEY-06 | Superadmin key inherits super_domains | unit | `cargo test -p mailrs-server api_key::tests::inherits_account_role -- --exact` | Wave 0 |
| AKEY-07 | Expired key returns 401 | unit | `cargo test -p mailrs-server api_key::tests::expired_key_rejected -- --exact` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p mailrs-server`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/server/src/api_key_store.rs` -- API key DB/cache operations + unit tests
- [ ] `crates/server/src/web/api_key.rs` -- CRUD handlers + integration tests
- [ ] `scripts/migrate-XXX-api-keys.sql` -- DB migration
- [ ] Tests requiring PG can use `#[sqlx::test]` or mock the store trait

## Integration Points (Critical for Planning)

### Files to Modify
| File | Change | Risk |
|------|--------|------|
| `crates/server/src/web/auth.rs` | 扩展 `AuthUser` struct + `from_request_parts` 添加 `mlrs_` 分支 | HIGH -- 所有认证流通过这里 |
| `crates/server/src/web/mod.rs` | 新增路由 + `SessionInfo` 可能重构 | MEDIUM |
| `crates/server/src/web/mod.rs` (`validate_domains`) | 适配新 `AuthUser` struct | MEDIUM |
| `crates/server/src/web/auth.rs` (`auth_me`) | 适配新 `AuthUser` struct | LOW |
| `scripts/init-schema.sql` | 添加 `api_keys` table（或单独 migration file） | LOW |

### Files to Create
| File | Purpose |
|------|---------|
| `crates/server/src/web/api_key.rs` | CRUD handlers |
| `crates/server/src/api_key_store.rs` | DB/cache operations |
| `scripts/migrate-XXX-api-keys.sql` | Database migration |

### Key Observation: AuthUser Refactor Scope

现有 `AuthUser` 是 tuple struct `AuthUser(pub String)`。要支持 AKEY-06（super_domains 继承），需要升级为 named fields struct。这会影响所有使用 `AuthUser(user)` 解构的地方。

Grep 统计影响范围：
- `auth.rs`: 2 处 (`AuthUser(user)` pattern)
- `auth_me`, `login` handlers
- `mail.rs`, `conversations.rs`, `admin.rs` 等 handler: 每个 handler 的 `AuthUser(user)` 参数

建议：分两步做。先改 `AuthUser` struct，修复所有编译错误（纯机械重构），再添加 API key 逻辑。

## Open Questions

1. **API key 创建是否只允许 session auth?**
   - What we know: 安全最佳实践是只允许 session auth 创建 key
   - What's unclear: 用户是否需要 API key 创建 API key（agent 自动化场景）
   - Recommendation: v1 只允许 session auth，v2 再评估

2. **AuthUser refactor 范围多大?**
   - What we know: ~20 处使用 `AuthUser(user)` 解构模式
   - What's unclear: 是否所有 handler 都需要 `super_domains`
   - Recommendation: `AuthUser` 加 `address` 字段，handler 用 `AuthUser { address, .. }` 解构，最小化改动

3. **Web UI 需要同步更新吗?**
   - What we know: API key 管理需要 web UI 页面
   - What's unclear: 是否 Phase 1 包含 web UI
   - Recommendation: Phase 1 只做 REST API，web UI 可后续添加。agent 场景不需要 UI

## Sources

### Primary (HIGH confidence)
- mailrs codebase: `auth.rs`, `mod.rs`, `domain_store.rs`, `users.rs` -- 直接代码审查
- mailrs codebase: `init-schema.sql`, `migrate-002-supermode.sql` -- DB schema
- mailrs codebase: `Cargo.toml` (server + workspace) -- 依赖版本确认

### Secondary (MEDIUM confidence)
- STACK.md, ARCHITECTURE.md, PITFALLS.md -- 项目前期研究文档
- REQUIREMENTS.md -- 需求定义

### Tertiary (LOW confidence)
- None -- 本阶段全部基于现有代码和已验证的前期研究

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- 全部是已有依赖，无新 crate
- Architecture: HIGH -- 扩展现有 AuthUser extractor，模式与 DomainStore 一致
- Pitfalls: HIGH -- 基于代码审查发现的具体问题（super_domains lookup, cache staleness）

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, no external dependency changes expected)
