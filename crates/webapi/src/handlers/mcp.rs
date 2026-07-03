//! MCP (Model Context Protocol) surface — fastcore-native.
//!
//! Mounts `/mcp` as a Streamable HTTP transport (rmcp 1.7) so AI
//! agents can drive mailrs the same way they drive the monolith's
//! /mcp used to be driven. Six core tools cover the daily flows:
//!
//! - `list_conversations` — inbox listing with pagination + filters
//! - `read_thread` — fetch every message in a thread
//! - `search_conversations` — free-text over subject + participants
//! - `mark_thread_read` — flip a thread's unread → seen
//! - `send_email` — compose + enqueue outbound
//! - `list_mailboxes` — folder enumeration
//!
//! Each session gets its own service instance; the authenticated user
//! flows in through a tokio task-local set by [`mcp_auth_middleware`].
//! Auth is Bearer-token only (same session cookie the web UI uses).
//!
//! The full 62-tool monolith surface (admin ops, queue management,
//! analysis, backfills) is a follow-up port; this covers the read/
//! write path end-to-end.

use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::WebState;

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListConversationsParams {
    /// Max threads to return. Defaults to 50, caps at 500.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Folder filter (e.g. "Sent", "INBOX"). Case-insensitive.
    #[serde(default)]
    pub folder: Option<String>,
    /// Category filter (e.g. "personal", "newsletter", "notifications").
    #[serde(default)]
    pub category: Option<String>,
    /// When true, restrict to threads with any unread message.
    #[serde(default)]
    pub unread_only: Option<bool>,
    /// Pagination cursor: return threads with `last_date < before_ts`.
    #[serde(default)]
    pub before_ts: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadThreadParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchConversationsParams {
    /// Free-text query. Case-insensitive; matches subject +
    /// participants + snippet.
    pub q: String,
    /// Max hits (default 20, cap 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkThreadReadParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendEmailParams {
    /// Recipient list.
    pub to: Vec<String>,
    /// Subject line.
    pub subject: String,
    /// Plain-text body. UTF-8.
    pub body: String,
    /// Optional Cc list.
    #[serde(default)]
    pub cc: Option<Vec<String>>,
    /// Optional From override — must be allowed by the caller's
    /// `send_as` permission or match their own address.
    #[serde(default)]
    pub from: Option<String>,
    /// Optional In-Reply-To Message-ID for threading.
    #[serde(default)]
    pub in_reply_to: Option<String>,
}

#[tool_router]
impl MailrsMcpService {
    pub fn new(state: Arc<WebState>, user: String) -> Self {
        Self {
            state,
            user,
            tool_router: Self::tool_router(),
        }
    }

    fn require_user(&self) -> Result<&str, McpError> {
        if self.user.is_empty() {
            return Err(McpError::invalid_params("not authenticated", None));
        }
        Ok(&self.user)
    }

    #[tool(
        description = "List conversations for the authenticated user. Supports folder / category / unread filters and cursor pagination."
    )]
    async fn list_conversations(
        &self,
        Parameters(params): Parameters<ListConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let limit = params.limit.unwrap_or(50).min(500);
        let req = mailrs_core_api::method::conversation::ListConversationsRequest {
            filter: mailrs_core_api::types::ConversationFilter {
                limit,
                before_ts: params.before_ts,
                category: params.category,
                domains: None,
                archived: false,
                folder: params.folder,
                unread: params.unread_only,
                starred: None,
                section: None,
            },
        };
        let resp = self
            .state
            .fast()
            .list_conversations(user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("list_conversations: {e}"), None))?;
        let items: Vec<_> = resp
            .items
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "snippet": c.snippet,
                    "participants": c.participants,
                    "last_date": c.last_date,
                    "unread_count": c.unread_count,
                    "message_count": c.message_count,
                    "category": c.category,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "conversations": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Fetch every message in a thread. Returns sender, recipients, subject, internal_date, and full text body per message."
    )]
    async fn read_thread(
        &self,
        Parameters(params): Parameters<ReadThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let resp = self
            .state
            .fast()
            .list_thread_messages(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("list_thread_messages: {e}"), None))?;
        let maildir_root =
            std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        let store = mailrs_message_store::MaildirStore;
        let mut items = Vec::with_capacity(resp.items.len());
        for w in resp.items {
            let msg = crate::handlers::conversations::enrich_with_body_public(
                &store,
                &maildir_root,
                user,
                w,
            )
            .await;
            items.push(serde_json::json!({
                "uid": msg.uid,
                "sender": msg.sender,
                "recipients": msg.recipients,
                "cc": msg.cc,
                "subject": msg.subject,
                "internal_date": msg.internal_date,
                "text_body": msg.text_body,
                "attachments": msg.attachments,
                "message_id": msg.message_id,
            }));
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "thread_id": params.thread_id, "messages": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Search conversations by free-text query. Matches subject + participants + snippet. Returns thread summaries."
    )]
    async fn search_conversations(
        &self,
        Parameters(params): Parameters<SearchConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let needle = params.q.to_lowercase();
        let limit = params.limit.unwrap_or(20).min(100);
        let req = mailrs_core_api::method::conversation::ListConversationsRequest {
            filter: mailrs_core_api::types::ConversationFilter {
                limit: 20_000,
                before_ts: None,
                category: None,
                domains: None,
                archived: false,
                folder: None,
                unread: None,
                starred: None,
                section: None,
            },
        };
        let resp = self
            .state
            .fast()
            .list_conversations(user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("list_conversations: {e}"), None))?;
        let matched: Vec<_> = resp
            .items
            .into_iter()
            .filter(|c| {
                c.subject.to_lowercase().contains(&needle)
                    || c.participants.to_lowercase().contains(&needle)
                    || c.snippet.to_lowercase().contains(&needle)
            })
            .take(limit as usize)
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "snippet": c.snippet,
                    "participants": c.participants,
                    "last_date": c.last_date,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "matches": matched }).to_string(),
        )]))
    }

    #[tool(description = "Mark a thread as read (zero its unread counter).")]
    async fn mark_thread_read(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .fast()
            .mark_thread_read(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("mark_thread_read: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "ok": true }).to_string(),
        )]))
    }

    #[tool(
        description = "Send an email. Enqueues to the outbound queue; delivery is asynchronous. Returns the assigned Message-ID."
    )]
    async fn send_email(
        &self,
        Parameters(params): Parameters<SendEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        if params.to.is_empty() {
            return Err(McpError::invalid_params("recipient list is empty", None));
        }
        let cc = params.cc.unwrap_or_default();
        if params.to.len() + cc.len() > 50 {
            return Err(McpError::invalid_params(
                "too many recipients (max 50)",
                None,
            ));
        }
        let from = params.from.unwrap_or_else(|| user.clone());
        let message_id = crate::handlers::prefs::send_email_mcp(
            &self.state,
            &user,
            &from,
            &params.to,
            &cc,
            &params.subject,
            &params.body,
            params.in_reply_to.as_deref(),
        )
        .await
        .map_err(|e| McpError::internal_error(format!("send: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "ok": true, "message_id": message_id }).to_string(),
        )]))
    }

    #[tool(description = "List every mailbox / folder the authenticated user has.")]
    async fn list_mailboxes(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let resp = self
            .state
            .fast()
            .list_mailboxes(user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_mailboxes: {e}"), None))?;
        let items: Vec<_> = resp
            .items
            .into_iter()
            .map(|m| serde_json::json!({ "id": m.id, "name": m.name, "uidnext": m.uidnext }))
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "mailboxes": items }).to_string(),
        )]))
    }
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
