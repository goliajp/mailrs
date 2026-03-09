pub(crate) mod auth;
pub(crate) mod tools;

use std::sync::Arc;

use rand_core::RngCore;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::ErrorData as McpError;
use rmcp::{tool, tool_handler, tool_router};

use crate::web::{AuthMethod, AuthUser, WebState};

tokio::task_local! {
    /// set by mcp_auth_middleware, read by the session factory closure
    pub(crate) static MCP_AUTH_USER: AuthUser;
}

use self::tools::{
    ListConversationsParams, ReadEmailParams, ReplyEmailParams, SearchEmailsParams,
    SendEmailParams,
};

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

    #[tool(description = "Send an email. Returns message ID on success.")]
    async fn send_email(
        &self,
        Parameters(params): Parameters<SendEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.to.is_empty() {
            return Err(McpError::invalid_params("recipient list is empty", None));
        }

        let cc = params.cc.unwrap_or_default();
        let total_recipients = params.to.len() + cc.len();
        if total_recipients > 50 {
            return Err(McpError::invalid_params(
                "too many recipients (max 50)",
                None,
            ));
        }

        let from = params
            .from
            .as_deref()
            .unwrap_or(&self.auth_user.address);

        if let Err(msg) = crate::web::mail::verify_sender(
            from,
            &self.auth_user.address,
            &self.auth_user.super_domains,
        ) {
            return Err(McpError::invalid_params(msg, None));
        }

        let now = chrono::Utc::now();
        let message_id = format!(
            "{}.{}@{}",
            now.timestamp_millis(),
            rand_core::OsRng.next_u32(),
            self.web_state.hostname,
        );

        let raw = crate::web::mail::build_rfc5322_message(
            from,
            &params.to,
            &cc,
            &params.subject,
            &params.body,
            params.html_body.as_deref(),
            &message_id,
            None,
            &[],
            &now,
            None,
        );

        let result = crate::web::mail::deliver_message(
            &self.web_state,
            from,
            &params.to,
            &cc,
            &[],
            &raw,
            &message_id,
            now.timestamp(),
        )
        .await;

        let body = result.0;
        if body.success {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({
                    "message_id": message_id,
                    "status": "queued",
                })
                .to_string(),
            )]))
        } else {
            Err(McpError::internal_error(
                body.message.unwrap_or_else(|| "delivery failed".to_string()),
                None,
            ))
        }
    }

    #[tool(description = "Read an email by UID. Returns sender, subject, text body, and metadata.")]
    async fn read_email(
        &self,
        Parameters(params): Parameters<ReadEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(ref mb_store) = self.web_state.mailbox_store else {
            return Err(McpError::internal_error(
                "mailbox store not available",
                None,
            ));
        };

        let user = &self.auth_user.address;
        let mailboxes = mb_store
            .list_mailboxes(user)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to list mailboxes: {e}"), None)
            })?;

        for mb in &mailboxes {
            if let Ok(Some(msg)) = mb_store.get_message(mb.id, params.uid).await {
                let raw = crate::message_util::read_message_raw(
                    &self.web_state.maildir_root,
                    user,
                    &msg.maildir_id,
                );
                let (text_body, _html_body, _attachments) = raw
                    .as_deref()
                    .map(crate::message_util::parse_message)
                    .unwrap_or_default();

                let sender = crate::message_util::decode_header(&msg.sender);
                let subject = crate::message_util::decode_header(&msg.subject);

                return Ok(CallToolResult::success(vec![Content::text(
                    serde_json::json!({
                        "uid": msg.uid,
                        "sender": sender,
                        "subject": subject,
                        "text_body": text_body.unwrap_or_default(),
                        "internal_date": msg.internal_date,
                        "message_id": msg.message_id,
                        "thread_id": msg.thread_id,
                    })
                    .to_string(),
                )]));
            }
        }

        Err(McpError::invalid_params("message not found", None))
    }

    #[tool(description = "Search emails by keyword. Returns conversation summaries (thread_id, subject, snippet, participants).")]
    async fn search_emails(
        &self,
        Parameters(params): Parameters<SearchEmailsParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(ref mb_store) = self.web_state.mailbox_store else {
            return Err(McpError::internal_error(
                "mailbox store not available",
                None,
            ));
        };

        let limit = params.limit.unwrap_or(20).min(20);
        let user = &self.auth_user.address;

        let results = mb_store
            .search_conversations(user, &params.query, limit, None, None)
            .await
            .map_err(|e| McpError::internal_error(format!("search failed: {e}"), None))?;

        let items: Vec<serde_json::Value> = results
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "snippet": c.snippet,
                    "participants": c.participants,
                    "last_date": c.last_date,
                    "message_count": c.message_count,
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string()),
        )]))
    }

    #[tool(description = "Reply to an email thread. Automatically sets In-Reply-To headers. Returns message ID.")]
    async fn reply_email(
        &self,
        Parameters(params): Parameters<ReplyEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(ref mb_store) = self.web_state.mailbox_store else {
            return Err(McpError::internal_error(
                "mailbox store not available",
                None,
            ));
        };

        let from = params
            .from
            .as_deref()
            .unwrap_or(&self.auth_user.address);

        if let Err(msg) = crate::web::mail::verify_sender(
            from,
            &self.auth_user.address,
            &self.auth_user.super_domains,
        ) {
            return Err(McpError::invalid_params(msg, None));
        }

        // resolve thread to get in_reply_to and references
        let (resolved_in_reply_to, references) = crate::web::mail::resolve_thread_reply(
            Some(&params.thread_id),
            None,
            from,
            Some(mb_store.as_ref()),
        )
        .await;

        let Some(ref in_reply_to) = resolved_in_reply_to else {
            return Err(McpError::invalid_params(
                "thread not found or has no messages",
                None,
            ));
        };

        // load thread messages to determine subject and reply recipient
        let thread_messages = mb_store
            .list_thread_messages(from, &params.thread_id, None)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to load thread: {e}"), None)
            })?;

        if thread_messages.is_empty() {
            return Err(McpError::invalid_params("thread has no messages", None));
        }

        let last_msg = &thread_messages[thread_messages.len() - 1];
        let subject = {
            let s = crate::message_util::decode_header(&last_msg.subject);
            if s.starts_with("Re: ") || s.starts_with("RE: ") || s.starts_with("re: ") {
                s
            } else {
                format!("Re: {s}")
            }
        };

        // reply to the sender of the last message
        let reply_to = crate::message_util::decode_header(&last_msg.sender);
        let to = vec![reply_to];

        let now = chrono::Utc::now();
        let message_id = format!(
            "{}.{}@{}",
            now.timestamp_millis(),
            rand_core::OsRng.next_u32(),
            self.web_state.hostname,
        );

        let raw = crate::web::mail::build_rfc5322_message(
            from,
            &to,
            &[],
            &subject,
            &params.body,
            None,
            &message_id,
            Some(in_reply_to),
            &references,
            &now,
            None,
        );

        let result = crate::web::mail::deliver_message(
            &self.web_state,
            from,
            &to,
            &[],
            &[],
            &raw,
            &message_id,
            now.timestamp(),
        )
        .await;

        let body = result.0;
        if body.success {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({
                    "message_id": message_id,
                    "thread_id": params.thread_id,
                    "status": "queued",
                })
                .to_string(),
            )]))
        } else {
            Err(McpError::internal_error(
                body.message.unwrap_or_else(|| "delivery failed".to_string()),
                None,
            ))
        }
    }

    #[tool(description = "List recent email conversations. Returns thread summaries (thread_id, subject, snippet, participants, unread count).")]
    async fn list_conversations(
        &self,
        Parameters(params): Parameters<ListConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(ref mb_store) = self.web_state.mailbox_store else {
            return Err(McpError::internal_error(
                "mailbox store not available",
                None,
            ));
        };

        let limit = params.limit.unwrap_or(20).min(20);
        let user = &self.auth_user.address;

        let results = mb_store
            .list_conversations(
                user,
                limit,
                None,
                params.category.as_deref(),
                None,
                false,
                None,
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to list conversations: {e}"), None)
            })?;

        let items: Vec<serde_json::Value> = results
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "snippet": c.snippet,
                    "participants": c.participants,
                    "message_count": c.message_count,
                    "unread_count": c.unread_count,
                    "last_date": c.last_date,
                    "category": c.category,
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string()),
        )]))
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
/// Auth approach: `mcp_auth_middleware` validates the Bearer token and sets
/// `MCP_AUTH_USER` (task-local) before calling `next.run(request)`. The
/// `StreamableHttpService` factory closure reads the task-local to create
/// `MailMcpService` with the correct authenticated user. Both run in the
/// same tokio task, so the task-local is always available in the factory.
pub fn setup_mcp(state: Arc<WebState>) -> axum::Router<Arc<WebState>> {
    let state_clone = state.clone();
    let service = StreamableHttpService::new(
        move || {
            // read auth user from task-local (set by mcp_auth_middleware)
            let auth_user = MCP_AUTH_USER
                .try_with(|u| u.clone())
                .unwrap_or_else(|_| AuthUser {
                    address: String::new(),
                    display_name: String::new(),
                    super_domains: vec![],
                    auth_method: AuthMethod::Session,
                });
            Ok(MailMcpService::new(state_clone.clone(), auth_user))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    axum::Router::new().nest_service("/mcp", service)
}
