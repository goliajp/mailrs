pub(crate) mod auth;
pub(crate) mod tools;

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;

use crate::web::{AuthUser, WebState};

/// MCP service that exposes mailrs operations as MCP tools
///
/// Each MCP session gets its own `MailMcpService` instance, created by the
/// factory closure in `setup_mcp`. The `auth_user` field is populated from
/// request extensions set by `mcp_auth_middleware`.
#[derive(Clone)]
pub(crate) struct MailMcpService {
    pub(crate) web_state: Arc<WebState>,
    pub(crate) auth_user: AuthUser,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl MailMcpService {
    pub(crate) fn new(web_state: Arc<WebState>, auth_user: AuthUser) -> Self {
        Self {
            web_state,
            auth_user,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler]
impl ServerHandler for MailMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
            .with_server_info(Implementation::new("mailrs", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "mailrs MCP server — tools for sending, reading, searching, and replying to emails.",
            )
    }
}

/// create the MCP axum Router
///
/// Auth approach: the factory closure for `StreamableHttpService` creates a
/// `MailMcpService` with a default/placeholder `AuthUser`. Since rmcp's
/// streamable HTTP transport doesn't expose per-request headers to the factory,
/// the `mcp_auth_middleware` validates the token and inserts `AuthUser` into
/// request extensions *before* the request reaches the MCP service.
///
/// For the initial implementation, each tool method will extract auth from
/// `self.auth_user` which is set during service construction. The middleware
/// is applied as a layer on the MCP router (will be wired in plan 02).
pub fn setup_mcp(state: Arc<WebState>) -> axum::Router<Arc<WebState>> {
    let state_clone = state.clone();
    let service = StreamableHttpService::new(
        move || {
            // default auth_user for the session factory
            // the actual auth is handled per-request by mcp_auth_middleware
            // and tool methods will use self.auth_user
            let default_user = AuthUser {
                address: String::new(),
                display_name: String::new(),
                super_domains: vec![],
                auth_method: crate::web::AuthMethod::Session,
            };
            Ok(MailMcpService::new(state_clone.clone(), default_user))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    axum::Router::new().nest_service("/mcp", service)
}
