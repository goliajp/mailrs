//! MCP (Model Context Protocol) surface — fastcore-native.
//!
//! Mounts `/mcp` as a Streamable HTTP transport (rmcp 1.7). Each
//! session gets its own service instance; the authenticated user
//! flows in through a tokio task-local set by [`mcp_auth_middleware`].
//! Auth is Bearer-token only (same session cookie the web UI uses).
//!
//! Tools live in per-category sub-modules with named routers
//! combined here via `Self::tool_router() = tool_router_v1() +
//! tool_router_v2_batch1() + ...`. Params moved to `params.rs`.
//!
//! v2.0.0 tool count = 37 (legacy `tool_router_v1`) + 25 (v2 batches
//! 1-10) = **62 total** — up from 37 in v1.9.x. Hits the plan
//! target. Batch layout:
//!   1. admin-read (groups/apps/email-groups/greylist/aliases)
//!   2. admin-misc (reconcile_maildir / list_scheduled / group members)
//!   3. per-user outbound control (cancel + reschedule scheduled)
//!   4. self-introspection (my permissions + own scheduled)
//!   5. encryption keys (own list + recipient public key)
//!   6. admin queue (list_admin_queue + list_failed_outbound)
//!   7. server info + retry_queue_message
//!   8. thread summary (get_thread_summary)
//!   9. thread mutations (snooze/unsnooze/pin/unpin/dismiss)
//!   10. dashboard metrics (get_inbox_metrics)

use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::tool_handler;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::WebState;

mod params;
mod tools_v1_read;
mod tools_v1_write;
mod tools_v2_batch1;
mod tools_v2_batch10;
mod tools_v2_batch11;
mod tools_v2_batch12;
mod tools_v2_batch13;
mod tools_v2_batch14;
mod tools_v2_batch2;
mod tools_v2_batch3;
mod tools_v2_batch4;
mod tools_v2_batch5;
mod tools_v2_batch6;
mod tools_v2_batch7;
mod tools_v2_batch8;
mod tools_v2_batch9;

tokio::task_local! {
    /// Set by `mcp_auth_middleware`, read by the session factory
    /// closure. When absent (unauthenticated call) the tool returns an
    /// invalid-params error rather than silently running as nobody.
    pub(crate) static MCP_AUTH_USER: String;
}

/// Per-session MCP service.
#[derive(Clone)]
pub struct MailrsMcpService {
    state: Arc<WebState>,
    user: String,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl MailrsMcpService {
    pub fn new(state: Arc<WebState>, user: String) -> Self {
        Self {
            state,
            user,
            tool_router: Self::tool_router(),
        }
    }

    /// Combined router: v1 (37 legacy tools) + v2 batch adds.
    fn tool_router() -> ToolRouter<Self> {
        Self::tool_router_v1_read()
            + Self::tool_router_v1_write()
            + Self::tool_router_v2_batch1()
            + Self::tool_router_v2_batch2()
            + Self::tool_router_v2_batch3()
            + Self::tool_router_v2_batch4()
            + Self::tool_router_v2_batch5()
            + Self::tool_router_v2_batch6()
            + Self::tool_router_v2_batch7()
            + Self::tool_router_v2_batch8()
            + Self::tool_router_v2_batch9()
            + Self::tool_router_v2_batch10()
            + Self::tool_router_v2_batch11()
            + Self::tool_router_v2_batch12()
            + Self::tool_router_v2_batch13()
            + Self::tool_router_v2_batch14()
    }
}

/// Shared `{ "ok": true }` success body for mutation tools.
pub(super) fn ok_result() -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        serde_json::json!({ "ok": true }).to_string(),
    )])
}

#[tool_handler]
impl ServerHandler for MailrsMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
            .with_server_info(Implementation::new("mailrs", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "mailrs email server MCP interface. list_conversations lists inbox, \
                 read_thread fetches every message in a thread, search_conversations \
                 does free-text over subject / participants / snippet, mark_thread_read \
                 flips a thread to seen, send_email enqueues outbound. Every tool acts \
                 as the authenticated user attached to the Bearer session.",
            )
    }
}

impl MailrsMcpService {
    /// Gate an admin tool: the authed user must carry an admin.*
    /// permission (or be super). Maps a FORBIDDEN to an MCP error.
    /// `pub(super)` so the sibling tool modules can call it.
    pub(super) async fn require_admin(&self, user: &str) -> Result<(), McpError> {
        crate::handlers::kevy_util::require_admin(&self.state, user)
            .await
            .map_err(|_| McpError::invalid_request("admin permission required", None))
    }

    pub(super) fn require_user(&self) -> Result<&str, McpError> {
        if self.user.is_empty() {
            return Err(McpError::invalid_params("not authenticated", None));
        }
        Ok(&self.user)
    }
}

/// Mount `/mcp` as a Streamable HTTP transport. Delegates auth to the
/// existing session middleware — an unauthenticated request hits the
/// tool with an empty user and gets an invalid-params error.
pub fn mcp_router(state: Arc<WebState>) -> axum::Router<Arc<WebState>> {
    // Same reasoning as monolith: MCP tools verify identity via the
    // task-local set by the auth middleware; DNS-rebinding host checks
    // don't add anything and break every hostname beyond localhost.
    let config = StreamableHttpServerConfig::default().disable_allowed_hosts();
    let state_for_factory = state.clone();
    let service = StreamableHttpService::new(
        move || {
            let user = MCP_AUTH_USER
                .try_with(|u| u.clone())
                .unwrap_or_else(|_| String::new());
            Ok(MailrsMcpService::new(state_for_factory.clone(), user))
        },
        LocalSessionManager::default().into(),
        config,
    );
    axum::Router::new().nest_service("/mcp", service)
}

/// Middleware that runs inside the MCP session future so that every
/// tool call sees `MCP_AUTH_USER`. Authenticates via the same session
/// path the REST endpoints use — an unauthenticated call runs with
/// an empty user string and the tools return "not authenticated".
pub async fn mcp_auth_middleware(
    axum::extract::State(_state): axum::extract::State<Arc<WebState>>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let user = crate::session::resolve_user_from_headers(req.headers())
        .await
        .unwrap_or_default();
    MCP_AUTH_USER.scope(user, next.run(req)).await
}
