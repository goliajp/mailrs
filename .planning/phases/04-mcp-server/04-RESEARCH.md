# Phase 4: MCP Server - Research

**Researched:** 2026-03-10
**Domain:** MCP (Model Context Protocol) server embedded in Rust/Axum
**Confidence:** HIGH

## Summary

Phase 4 将 MCP server 嵌入 mailrs-server 进程，通过 `/mcp` 端点暴露邮件操作工具。核心依赖是 `rmcp` crate（official Rust SDK for MCP，v1.1.1），它提供 `StreamableHttpService` 可直接作为 Tower service 嵌入现有 axum Router。

rmcp 的 `#[tool_router]` + `#[tool]` 宏提供声明式工具定义，`StreamableHttpService::new()` 接受 factory closure 创建 per-session handler 实例。认证方面，mailrs 已有 `verify_api_key` 逻辑，MCP 工具可通过 axum middleware 在 `/mcp` 路由前验证 Bearer token，然后通过 rmcp 的 extensions 机制将 `AuthUser` 传递给工具 handler。

**Primary recommendation:** 使用 rmcp 1.1 的 `transport-streamable-http-server` feature，通过 `nest_service("/mcp", service)` 嵌入现有 router，认证复用现有 API key Bearer token 中间件。

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MCP-01 | MCP server embedded in mailrs-server using Rust rmcp | rmcp 1.1.1 `StreamableHttpService` 可直接 `nest_service` 到 axum Router |
| MCP-02 | Streamable HTTP transport mounted at `/mcp` route | `axum::Router::new().nest_service("/mcp", service)` 模式已验证 |
| MCP-03 | send_email tool available via MCP | 复用 `verify_sender` + `send_message` 逻辑，通过 `#[tool]` 宏暴露 |
| MCP-04 | read_email tool available via MCP | 复用 `get_message` / `get_thread_messages` 逻辑 |
| MCP-05 | search_emails tool available via MCP | 复用 `search_conversations` 逻辑 |
| MCP-06 | reply_email tool available via MCP | 复用 `send_message` + `reply_to_thread_id` 字段 |
| MCP-07 | list_conversations tool available via MCP | 复用 `get_conversations` 逻辑 |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 1.1.1 | Official Rust MCP SDK | Official SDK by modelcontextprotocol org, 内建 axum 集成 |
| schemars | 1.0 | JSON Schema for tool params | rmcp `#[tool]` 宏要求参数结构体 derive `JsonSchema` |
| axum | 0.8 (existing) | HTTP framework | 已有依赖，rmcp 1.1 兼容 axum 0.8 |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-util | 0.7 (existing) | CancellationToken | 优雅关闭 MCP sessions |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rmcp | rust-mcp-sdk | rmcp 是 official SDK，社区更活跃 |
| Streamable HTTP | stdio wrapper | 需要独立进程，无法复用 WebState |

**Installation:**
```bash
# add to crates/server/Cargo.toml
cargo add rmcp --features "server,macros,transport-streamable-http-server,schemars" -p mailrs-server
cargo add schemars@1.0 -p mailrs-server
```

## Architecture Patterns

### Recommended Project Structure
```
crates/server/src/
├── mcp/
│   ├── mod.rs           # MCP service struct + ServerHandler impl + router setup
│   ├── tools.rs         # #[tool] method implementations (send, read, search, reply, list)
│   └── auth.rs          # MCP auth middleware (reuses verify_api_key)
├── web/
│   └── mod.rs           # router() adds .nest_service("/mcp", mcp_service)
└── ...
```

### Pattern 1: Tool Router with Factory Pattern
**What:** rmcp 使用 factory closure 为每个 MCP session 创建独立 handler 实例
**When to use:** 创建 StreamableHttpService 时
**Example:**
```rust
// source: rmcp official examples + shuttle blog
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::{ServerHandler, ServerInfo};
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

#[derive(Clone)]
struct MailMcpService {
    web_state: Arc<WebState>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl MailMcpService {
    fn new(web_state: Arc<WebState>) -> Self {
        Self {
            web_state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Send an email")]
    async fn send_email(
        &self,
        #[tool(aggr)] req: SendEmailParams,
    ) -> Result<CallToolResult, McpError> {
        // reuse existing send logic via web_state
    }
}

#[tool_handler]
impl ServerHandler for MailMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "mailrs".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some("Mail server MCP tools for sending, reading, and searching emails.".to_string()),
        }
    }
}
```

### Pattern 2: Auth Middleware for MCP Route
**What:** 在 nest_service 前添加 axum middleware 验证 Bearer token
**When to use:** MCP 认证
**Example:**
```rust
// approach: axum middleware layer on /mcp route
let state_for_mcp = state.clone();
let mcp_service = StreamableHttpService::new(
    move || Ok(MailMcpService::new(state_for_mcp.clone())),
    LocalSessionManager::default().into(),
    Default::default(),
);

// wrap with auth middleware before nesting
let mcp_router = axum::Router::new()
    .nest_service("/mcp", mcp_service)
    .layer(middleware::from_fn_with_state(
        state.clone(),
        mcp_auth_middleware,
    ));

// merge into main app
app.merge(mcp_router)
```

### Pattern 3: Tool Parameter Structs with JsonSchema
**What:** 每个 MCP tool 的参数用 `schemars::JsonSchema` derive 的 struct
**When to use:** 所有工具定义
**Example:**
```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SendEmailParams {
    #[schemars(description = "Sender email address")]
    from: String,
    #[schemars(description = "Recipient email addresses")]
    to: Vec<String>,
    #[schemars(description = "Email subject")]
    subject: String,
    #[schemars(description = "Plain text email body")]
    body: String,
    #[schemars(description = "Optional HTML email body")]
    html_body: Option<String>,
}
```

### Anti-Patterns to Avoid
- **在 MCP tool handler 中直接操作 DB:** 应复用现有 `MailboxStore` / `send_message` 逻辑，不要绕过业务层
- **每个 tool 独立做认证:** 认证应在 middleware 层统一处理，tool handler 内只关注业务逻辑
- **返回 HTML 内容给 AI:** MCP tool 结果应返回纯文本或 JSON，不要返回 HTML body

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP protocol handling | 自写 JSON-RPC + SSE | rmcp `StreamableHttpService` | MCP 协议复杂（session 管理、SSE streaming、JSON-RPC batch） |
| Tool schema generation | 手写 JSON Schema | `schemars` derive macro | 参数类型多，手写易出错 |
| Session management | 自写 session 存储 | `LocalSessionManager` | rmcp 内建，支持 `Mcp-Session-Id` header |
| Email sending logic | 在 MCP handler 重写 | 复用 `web::mail::send_message` 核心逻辑 | 避免重复代码、保持一致性 |

**Key insight:** MCP tools 应该是现有 REST API 逻辑的薄封装层，不应引入新的业务逻辑。

## Common Pitfalls

### Pitfall 1: Factory Closure 中的 State 共享
**What goes wrong:** `StreamableHttpService::new()` 的 factory closure 每次创建新 session 时调用，如果不 clone `Arc<WebState>` 会编译失败
**Why it happens:** Rust 的 move 语义要求 closure 拥有数据
**How to avoid:** 在 closure 外 clone `Arc<WebState>`，closure 内使用 clone
**Warning signs:** `cannot move out of captured variable` 编译错误

### Pitfall 2: MCP 认证与现有 AuthUser 的集成
**What goes wrong:** rmcp 的 `StreamableHttpService` 是 Tower service，不走 axum 的 `FromRequestParts` extractor 路径
**Why it happens:** `nest_service` 将整个子树委托给 Tower service，axum extractors 不适用
**How to avoid:** 使用 axum middleware 在 request 到达 MCP service 之前验证 token，将 auth info 注入 request extensions；或者在 MCP tool handler 中手动验证
**Warning signs:** 工具调用不需认证也能成功

### Pitfall 3: 返回值过大导致 token 耗尽
**What goes wrong:** `list_conversations` 或 `search_emails` 返回太多数据，超出 MCP client 的 token 限制
**Why it happens:** REST API 返回完整 JSON 列表，但 AI agent 的 context window 有限
**How to avoid:** MCP tool 应硬编码较小的 limit（如 20 条），返回摘要而非全文
**Warning signs:** Claude Code 显示 "MCP tool output exceeds 10,000 tokens" 警告

### Pitfall 4: Tool Description 不够清晰
**What goes wrong:** AI agent 选错工具或传错参数
**Why it happens:** `#[tool(description = "...")]` 和 `#[schemars(description = "...")]` 描述模糊
**How to avoid:** 提供详细的工具说明和参数描述，包含示例值
**Warning signs:** Agent 反复调用错误工具

### Pitfall 5: reply_email 工具缺少 thread context
**What goes wrong:** Agent 回复邮件时没有正确设置 In-Reply-To header
**Why it happens:** 只传 thread_id 但忘记查询原始 message_id
**How to avoid:** reply_email 工具内部先查询 thread 最后一条消息的 message_id，自动设置 in_reply_to
**Warning signs:** 回复邮件不出现在同一 thread 中

## Code Examples

### MCP Service 嵌入 Axum Router
```rust
// source: rmcp official examples (counter_streamhttp.rs)
pub fn setup_mcp(state: Arc<WebState>) -> axum::Router {
    let state_clone = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(MailMcpService::new(state_clone.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    axum::Router::new()
        .nest_service("/mcp", service)
}
```

### Claude Code 配置
```bash
# add mailrs MCP server to Claude Code
claude mcp add --transport http mailrs https://mail.golia.jp/mcp \
  --header "Authorization: Bearer mlrs_xxxxxxxx_yyyyyyyyyyyyyyyyyyyy"
```

Or in `.mcp.json`:
```json
{
  "mcpServers": {
    "mailrs": {
      "type": "http",
      "url": "https://mail.golia.jp/mcp",
      "headers": {
        "Authorization": "Bearer ${MAILRS_API_KEY}"
      }
    }
  }
}
```

### Tool Result 返回格式
```rust
// source: rmcp official examples
use rmcp::model::{CallToolResult, Content};

// success
Ok(CallToolResult::success(vec![Content::text(
    serde_json::to_string_pretty(&result).unwrap(),
)]))

// error
Err(McpError::invalid_params("recipient list is empty", None))
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SSE transport | Streamable HTTP | MCP spec 2025-03-26 | SSE 已 deprecated，新实现用 Streamable HTTP |
| `#[tool_box]` macro | `#[tool_router]` macro | rmcp ~0.8+ | tool_router 是新 API，替代旧 tool_box |
| 独立 MCP 进程 (stdio) | 嵌入式 HTTP transport | 2025 | 可复用服务内部状态，无需 IPC |

**Deprecated/outdated:**
- SSE transport: MCP spec 已 deprecate，用 Streamable HTTP 替代
- `tool_box!` macro: rmcp 旧 API，使用 `#[tool_router]` 替代

## Open Questions

1. **rmcp 1.1 + axum 0.8 编译兼容性**
   - What we know: rmcp 1.1.1 声明兼容 axum 0.8，项目当前用 axum 0.8.8
   - What's unclear: 实际编译时是否有依赖冲突（STATE.md 中标记为 blocker）
   - Recommendation: Wave 0 第一步 `cargo check` 验证

2. **MCP 认证传递机制**
   - What we know: 可通过 axum middleware 或 rmcp extensions 传递 auth info
   - What's unclear: rmcp 的 `on_request_fn` 在 axum transport 中是否可用（文档主要展示 actix-web）
   - Recommendation: 优先用 axum middleware 方案（更简单），如不行再用 factory closure 中做认证

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | none (workspace Cargo.toml) |
| Quick run command | `cargo test -p mailrs-server -- mcp` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-01 | MCP service 可创建并挂载 | unit | `cargo test -p mailrs-server -- mcp::tests -x` | Wave 0 |
| MCP-02 | /mcp 端点响应 MCP protocol | integration | manual (需要完整 server) | Wave 0 |
| MCP-03 | send_email tool 可发送邮件 | unit | `cargo test -p mailrs-server -- mcp::tools::tests::send -x` | Wave 0 |
| MCP-04 | read_email tool 可读取邮件 | unit | `cargo test -p mailrs-server -- mcp::tools::tests::read -x` | Wave 0 |
| MCP-05 | search_emails tool 可搜索 | unit | `cargo test -p mailrs-server -- mcp::tools::tests::search -x` | Wave 0 |
| MCP-06 | reply_email tool 可回复 | unit | `cargo test -p mailrs-server -- mcp::tools::tests::reply -x` | Wave 0 |
| MCP-07 | list_conversations tool 可列出 | unit | `cargo test -p mailrs-server -- mcp::tools::tests::list -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p mailrs-server -- mcp -x`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/server/src/mcp/mod.rs` -- MCP service struct + ServerHandler
- [ ] `crates/server/src/mcp/tools.rs` -- tool implementations + unit tests
- [ ] `crates/server/src/mcp/auth.rs` -- auth middleware for MCP route
- [ ] `Cargo.toml` dependency: `rmcp` + `schemars`

## Sources

### Primary (HIGH confidence)
- [rmcp 1.1.1 on docs.rs](https://docs.rs/crate/rmcp/latest) - version, features, API
- [rmcp official GitHub (modelcontextprotocol/rust-sdk)](https://github.com/modelcontextprotocol/rust-sdk) - examples, README
- [Shuttle blog: Streamable HTTP MCP Server in Rust](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp) - complete code walkthrough
- [Claude Code MCP docs](https://code.claude.com/docs/en/mcp) - client-side configuration format
- [MCP Spec Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) - Streamable HTTP protocol

### Secondary (MEDIUM confidence)
- [HackMD rmcp guide](https://hackmd.io/@Hamze/S1tlKZP0kx) - tool macro patterns
- [rmcp simple_auth_streamhttp example](https://github.com/modelcontextprotocol/rust-sdk/tree/main/examples/servers) - auth middleware pattern

### Tertiary (LOW confidence)
- rmcp `on_request_fn` for axum (only verified for actix-web transport)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - rmcp 是 official SDK，v1.1.1 发布于 2026-03-09，axum 0.8 兼容性声明明确
- Architecture: HIGH - `nest_service` + factory pattern 有多个官方示例验证
- Pitfalls: MEDIUM - 认证传递机制在 axum transport 的文档不如 actix-web 充分

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (rmcp 迭代较快，30 天内可能有 minor 更新)
