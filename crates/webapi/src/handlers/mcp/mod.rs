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
//!   1. admin-read (groups/email-groups/greylist/aliases)
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
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{RoleServer, tool_handler};

use crate::WebState;
pub use scopes::McpIdentity;

mod params;
mod params_greylist;
mod scopes;
mod tools_mock;
mod tools_v1_read;
mod tools_v1_write;
mod tools_v2_batch1;
mod tools_v2_batch10;
mod tools_v2_batch11;
mod tools_v2_batch12;
mod tools_v2_batch13;
mod tools_v2_batch14;
mod tools_v2_batch15;
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
    pub(crate) static MCP_AUTH_USER: McpIdentity;
}

/// Per-session MCP service.
#[derive(Clone)]
pub struct MailrsMcpService {
    state: Arc<WebState>,
    identity: McpIdentity,
    tool_router: ToolRouter<Self>,
}

impl MailrsMcpService {
    pub fn new(state: Arc<WebState>, identity: McpIdentity) -> Self {
        Self {
            state,
            identity,
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
            + Self::tool_router_v2_batch15()
            + Self::tool_router_mock()
    }
}

/// Shared `{ "ok": true }` success body for mutation tools.
pub(super) fn ok_result() -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::json!({ "ok": true }).to_string(),
    )])
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MailrsMcpService {
    /// Every tool call passes the scope gate before the router runs it.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.identity
            .check_tool(&request.name, request.arguments.as_ref())?;
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
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
    /// What the REST route `template` needs (`route_permissions`): a tool
    /// that does what a route does asks the same of its caller.
    pub(super) async fn require_route(&self, user: &str, template: &str) -> Result<(), McpError> {
        use crate::handlers::route_permissions::{Need, need};
        let ok = match need(template) {
            Need::Permission(p) => {
                crate::handlers::kevy_util::require_permission(&self.state, user, p).await
            }
            Need::AnyAdmin => crate::handlers::kevy_util::require_admin(&self.state, user).await,
            Need::Super => crate::handlers::kevy_util::require_super(&self.state, user).await,
        };
        ok.map_err(|_| McpError::invalid_request(format!("not permitted ({template})"), None))
    }

    pub(super) async fn require_super(&self, user: &str) -> Result<(), McpError> {
        crate::handlers::kevy_util::require_super(&self.state, user)
            .await
            .map_err(|_| McpError::invalid_request("super administrator required", None))
    }

    /// A recent password confirmation by the session behind this call. An
    /// agent key has no password and is refused.
    pub(super) fn require_confirmed(&self) -> Result<crate::session::confirm::Confirmed, McpError> {
        crate::session::confirm::require(self.identity.session.as_ref())
            .map_err(|r| McpError::invalid_request(r.code(), None))
    }

    pub(super) fn require_user(&self) -> Result<&str, McpError> {
        if self.identity.user.is_empty() {
            return Err(McpError::invalid_params("not authenticated", None));
        }
        Ok(&self.identity.user)
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
            let identity = MCP_AUTH_USER.try_with(|i| i.clone()).unwrap_or_default();
            Ok(MailrsMcpService::new(state_for_factory.clone(), identity))
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
    let auth = crate::session::resolve_auth_from_headers(req.headers()).await;
    // a send-only key has no tool it may call
    if auth
        .as_ref()
        .is_some_and(|a| crate::session::send_only(&a.scopes))
    {
        return axum::response::IntoResponse::into_response(axum::http::StatusCode::FORBIDDEN);
    }
    let identity = auth
        .map(|a| McpIdentity {
            session: a.session,
            ..McpIdentity::new(a.user, a.scopes)
        })
        .unwrap_or_default();
    MCP_AUTH_USER.scope(identity, next.run(req)).await
}
