//! CARVE-OUT: this file intentionally exceeds the 500-LOC project
//! limit — it holds the v1 tool set that predates the named-router
//! split. New tools go into `tools_parityN.rs` sub-modules, each with
//! its own `#[tool_router(router = tool_router_parityN)]`; the routers
//! are summed in [`MailMcpService::tool_router`]. Per-tool param types
//! for the v1 set live next door in `tools.rs`.

pub(crate) mod auth;
pub(crate) mod tools;
mod tools_parity1;
mod tools_parity2;
mod tools_parity3;
mod tools_parity4;
mod tools_parity5;
mod tools_parity6;
mod tools_v1_directory;
mod tools_v1_directory_write;
mod tools_v1_email_groups;
mod tools_v1_mail;
mod tools_v1_mail_read;
mod tools_v1_ops;
mod tools_v1_ops_audit;

use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool_handler, tool_router};

use base64::Engine;

use crate::web::{AuthMethod, AuthUser, WebState};

tokio::task_local! {
    /// set by mcp_auth_middleware, read by the session factory closure
    pub(crate) static MCP_AUTH_USER: AuthUser;
}

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

impl MailMcpService {
    /// Combined router: v1 (this file) + the parity batches that bring
    /// the monolith lane level with fastcore.
    fn tool_router() -> ToolRouter<Self> {
        Self::tool_router_v1()
            + Self::tool_router_v1_mail()
            + Self::tool_router_v1_directory()
            + Self::tool_router_v1_ops()
            + Self::tool_router_v1_mail_read()
            + Self::tool_router_v1_directory_write()
            + Self::tool_router_v1_ops_audit()
            + Self::tool_router_v1_email_groups()
            + Self::tool_router_parity1()
            + Self::tool_router_parity2()
            + Self::tool_router_parity3()
            + Self::tool_router_parity4()
            + Self::tool_router_parity5()
            + Self::tool_router_parity6()
    }
}

#[tool_router(router = tool_router_v1, vis = "pub(crate)")]
impl MailMcpService {
    pub(crate) fn new(web_state: Arc<WebState>, auth_user: AuthUser) -> Self {
        Self {
            web_state,
            auth_user,
            tool_router: Self::tool_router(),
        }
    }

    // --- admin / user management tools ---

    // --- shared helpers ---

    fn ds(&self) -> Result<&Arc<crate::domain_store::DomainStore>, McpError> {
        self.web_state
            .domain_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))
    }

    fn pool(&self) -> Result<&crate::pg::BackendPool, McpError> {
        self.web_state
            .pg_pool
            .as_ref()
            .ok_or_else(|| McpError::internal_error("database unavailable", None))
    }

    fn mb_store(&self) -> Result<&Arc<mailrs_mailbox::PgMailboxStore>, McpError> {
        self.web_state
            .mailbox_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("mailbox store not available", None))
    }

    fn pg_pool(&self) -> Result<&crate::pg::BackendPool, McpError> {
        self.web_state
            .pg_pool
            .as_ref()
            .ok_or_else(|| McpError::internal_error("database not configured", None))
    }

    fn outbound_pool(&self) -> Result<&crate::pg::BackendPool, McpError> {
        self.web_state
            .outbound_queue
            .as_ref()
            .ok_or_else(|| McpError::internal_error("outbound queue not configured", None))
    }

    fn json_result(&self, items: &[serde_json::Value]) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(items).unwrap_or_else(|_| "[]".into()),
        )]))
    }

    fn ok_result(&self, status: &str, detail: &str) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": status, "detail": detail}).to_string(),
        )]))
    }

    fn require_permission(&self, perm: &str) -> Result<(), McpError> {
        if self.auth_user.permissions.has(perm) {
            Ok(())
        } else {
            Err(McpError::invalid_params(
                format!("insufficient permissions: requires {perm}"),
                None,
            ))
        }
    }

    fn validate_audit_target(&self, target_user: &str) -> Result<(), McpError> {
        let domain = target_user.split_once('@').map(|(_, d)| d).unwrap_or("");
        if domain.is_empty() {
            return Err(McpError::invalid_params(
                "invalid target user address",
                None,
            ));
        }
        let perms = &self.auth_user.permissions;
        let accessible = perms.accessible_domains();
        if !perms.is_super() && !accessible.iter().any(|d| d == domain) {
            return Err(McpError::invalid_params(
                "target user not in accessible domains",
                None,
            ));
        }
        Ok(())
    }

    // --- domain management ---

    // --- alias management ---

    // --- greylist local lists (Phase 2) ---

    // --- app management ---

    // --- webhook management ---

    // --- mail operations ---

    // --- queue management ---

    // --- email group management ---

    // --- signature management (user-level, no admin required) ---

    // --- encryption key management ---

    // --- mail audit ---

    // --- system config tools ---
}

#[tool_handler]
impl ServerHandler for MailMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
            .with_server_info(Implementation::new("mailrs", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "mailrs MCP server — tools for email operations (send, read, search, reply) and account/permission management (create/remove accounts, manage group memberships). Admin tools require appropriate permissions.",
            )
    }
}

const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024; // 25 MB

/// resolve attachment data field: URL → download, existing file → read, otherwise → base64 decode
pub(super) async fn resolve_attachments(
    attachments: Vec<tools::Attachment>,
) -> Result<Vec<crate::web::mail::AttachmentData>, McpError> {
    let mut results = Vec::with_capacity(attachments.len());

    for a in attachments {
        let data_str = a.data.trim();

        // determine source type
        let (bytes, derived_filename) = if data_str.starts_with("http://")
            || data_str.starts_with("https://")
        {
            // URL: download
            let resp = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| McpError::internal_error(format!("http client: {e}"), None))?
                .get(data_str)
                .send()
                .await
                .map_err(|e| {
                    McpError::invalid_params(
                        format!("failed to download attachment '{}': {e}", data_str),
                        None,
                    )
                })?;
            if !resp.status().is_success() {
                return Err(McpError::invalid_params(
                    format!("download failed for '{}': HTTP {}", data_str, resp.status()),
                    None,
                ));
            }
            let bytes = resp.bytes().await.map_err(|e| {
                McpError::invalid_params(format!("failed to read response body: {e}"), None)
            })?;
            if bytes.len() > MAX_ATTACHMENT_SIZE {
                return Err(McpError::invalid_params(
                    format!(
                        "attachment too large: {} bytes (max {})",
                        bytes.len(),
                        MAX_ATTACHMENT_SIZE
                    ),
                    None,
                ));
            }
            // derive filename from URL path
            let name = url_filename(data_str);
            (bytes.to_vec(), name)
        } else if tokio::fs::metadata(data_str).await.is_ok() {
            // file path: read from disk
            let bytes = tokio::fs::read(data_str).await.map_err(|e| {
                McpError::invalid_params(format!("failed to read file '{}': {e}", data_str), None)
            })?;
            if bytes.len() > MAX_ATTACHMENT_SIZE {
                return Err(McpError::invalid_params(
                    format!(
                        "attachment too large: {} bytes (max {})",
                        bytes.len(),
                        MAX_ATTACHMENT_SIZE
                    ),
                    None,
                ));
            }
            let name = std::path::Path::new(data_str)
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from);
            (bytes, name)
        } else {
            // base64
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data_str)
                .map_err(|_| {
                    McpError::invalid_params(
                        "attachment data is not a valid URL, file path, or base64 string"
                            .to_string(),
                        None,
                    )
                })?;
            if bytes.len() > MAX_ATTACHMENT_SIZE {
                return Err(McpError::invalid_params(
                    format!(
                        "attachment too large: {} bytes (max {})",
                        bytes.len(),
                        MAX_ATTACHMENT_SIZE
                    ),
                    None,
                ));
            }
            (bytes, None)
        };

        let filename = a
            .filename
            .or(derived_filename)
            .unwrap_or_else(|| "attachment".to_string());

        let content_type = a.content_type.unwrap_or_else(|| {
            mime_guess::from_path(&filename)
                .first_raw()
                .unwrap_or("application/octet-stream")
                .to_string()
        });

        results.push(crate::web::mail::AttachmentData {
            filename,
            content_type,
            data: bytes,
        });
    }

    Ok(results)
}

/// extract filename from URL path segment
fn url_filename(url: &str) -> Option<String> {
    url.split('?')
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|s| !s.is_empty() && s.contains('.'))
        .map(|s| {
            // url-decode percent-encoded characters
            urlencoding::decode(s)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| s.to_string())
        })
}

/// create the MCP axum Router
///
/// Auth approach: `mcp_auth_middleware` validates the Bearer token and sets
/// `MCP_AUTH_USER` (task-local) before calling `next.run(request)`. The
/// `StreamableHttpService` factory closure reads the task-local to create
/// `MailMcpService` with the correct authenticated user. Both run in the
/// same tokio task, so the task-local is always available in the factory.
pub fn setup_mcp(state: Arc<WebState>) -> axum::Router<Arc<WebState>> {
    // rmcp 1.7 ships DNS-rebinding protection that 403s any request whose
    // Host header is outside `allowed_hosts` (library default:
    // localhost-only — which silently broke every public-hostname MCP
    // client with "Forbidden: Host header is not allowed").
    //
    // Disabled here deliberately: that protection exists for
    // UNauthenticated local servers a browser could reach via DNS
    // rebinding. mailrs's /mcp requires a Bearer token on every call
    // (mcp_auth_middleware); a rebound origin without a token gets an
    // empty-permission AuthUser and can do nothing. Host checks add zero
    // security here and one more way to break clients (rename the host,
    // add a domain, hit via internal IP).
    let config = StreamableHttpServerConfig::default().disable_allowed_hosts();

    let state_clone = state.clone();
    let service = StreamableHttpService::new(
        move || {
            metrics::counter!("mailrs_mcp_sessions_total").increment(1);
            // read auth user from task-local (set by mcp_auth_middleware)
            let auth_user = MCP_AUTH_USER
                .try_with(|u| u.clone())
                .unwrap_or_else(|_| AuthUser {
                    address: String::new(),
                    display_name: String::new(),
                    permissions: std::sync::Arc::new(
                        crate::permission::compute_effective_permissions(&[], &[], &[]),
                    ),
                    auth_method: AuthMethod::Session,
                });
            Ok(MailMcpService::new(state_clone.clone(), auth_user))
        },
        LocalSessionManager::default().into(),
        config,
    );

    axum::Router::new().nest_service("/mcp", service)
}
