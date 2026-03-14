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

use base64::Engine;

use crate::web::{AuthMethod, AuthUser, WebState};

tokio::task_local! {
    /// set by mcp_auth_middleware, read by the session factory closure
    pub(crate) static MCP_AUTH_USER: AuthUser;
}

use self::tools::{
    AddAccountToGroupParams, CreateAccountParams, GetAccountPermissionsParams,
    ListAccountsParams, ListConversationsParams, ListGroupsParams, ReadEmailParams,
    RemoveAccountFromGroupParams, RemoveAccountParams, ReplyEmailParams, SearchEmailsParams,
    SendEmailParams, SetAccountPasswordParams,
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
            &self.auth_user.permissions,
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

        // decode base64 attachments
        let attachment_data: Vec<crate::web::mail::AttachmentData> = params
            .attachments
            .unwrap_or_default()
            .into_iter()
            .map(|a| {
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&a.data)
                    .map_err(|_| {
                        McpError::invalid_params(
                            format!("invalid base64 in attachment '{}'", a.filename),
                            None,
                        )
                    })?;
                Ok(crate::web::mail::AttachmentData {
                    filename: a.filename,
                    content_type: a.content_type,
                    data,
                })
            })
            .collect::<Result<Vec<_>, McpError>>()?;

        let raw = crate::web::mail::build_rfc5322_with_attachments(
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
            &attachment_data,
            None,
            &[],
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
            &self.auth_user.permissions,
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

    // --- admin / user management tools ---

    /// helper: check permission and return MCP error if denied
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

    #[tool(description = "List all email accounts. Requires admin.accounts permission. Returns address, domain, display_name, active status.")]
    async fn list_accounts(
        &self,
        Parameters(_params): Parameters<ListAccountsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let accounts = ds.list_accounts().await
            .map_err(|e| McpError::internal_error(format!("failed to list accounts: {e}"), None))?;

        let items: Vec<serde_json::Value> = accounts.into_iter().map(|a| {
            serde_json::json!({
                "address": a.address,
                "domain": a.domain,
                "display_name": a.display_name,
                "active": a.active,
            })
        }).collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()),
        )]))
    }

    #[tool(description = "Create a new email account (onboarding). Requires admin.accounts permission. Automatically adds account to the domain's default user group.")]
    async fn create_account(
        &self,
        Parameters(params): Parameters<CreateAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let password_hash = if params.password.is_empty() {
            String::new()
        } else {
            crate::users::UserStore::hash_password(&params.password)
                .unwrap_or_else(|_| params.password.clone())
        };

        ds.add_account(&params.address, &params.domain, &params.display_name, &password_hash, 0)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to create account: {e}"), None))?;

        // auto-add to domain's user group
        let groups = ds.list_groups(Some(&params.domain)).await.unwrap_or_default();
        if let Some(user_group) = groups.iter().find(|g| g.name == "user" && g.domain.as_deref() == Some(&params.domain)) {
            let _ = ds.add_account_to_group(&params.address, user_group.id).await;
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "created", "address": params.address}).to_string(),
        )]))
    }

    #[tool(description = "Remove an email account (offboarding). Requires admin.accounts permission. Removes account, group memberships, and permission overrides.")]
    async fn remove_account(
        &self,
        Parameters(params): Parameters<RemoveAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let removed = ds.remove_account(&params.address).await
            .map_err(|e| McpError::internal_error(format!("failed to remove account: {e}"), None))?;

        if removed {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"status": "removed", "address": params.address}).to_string(),
            )]))
        } else {
            Err(McpError::invalid_params("account not found", None))
        }
    }

    #[tool(description = "Reset an account's password. Requires admin.accounts permission.")]
    async fn set_account_password(
        &self,
        Parameters(params): Parameters<SetAccountPasswordParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let password_hash = crate::users::UserStore::hash_password(&params.password)
            .map_err(|e| McpError::internal_error(format!("failed to hash password: {e}"), None))?;

        // re-add account with new password (upsert)
        let existing = ds.get_account_with_hash(&params.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?
            .ok_or_else(|| McpError::invalid_params("account not found", None))?;

        ds.add_account(
            &params.address,
            &existing.0.domain,
            &existing.0.display_name,
            &password_hash,
            0,
        ).await
            .map_err(|e| McpError::internal_error(format!("failed to update password: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "password_updated", "address": params.address}).to_string(),
        )]))
    }

    #[tool(description = "List all permission groups. Returns group id, name, domain (null=global), and builtin flag.")]
    async fn list_groups(
        &self,
        Parameters(_params): Parameters<ListGroupsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let groups = ds.list_groups(None).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let items: Vec<serde_json::Value> = groups.into_iter().map(|g| {
            serde_json::json!({
                "id": g.id,
                "name": g.name,
                "domain": g.domain,
                "is_builtin": g.is_builtin,
                "description": g.description,
            })
        }).collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()),
        )]))
    }

    #[tool(description = "Get groups and effective permissions for an account. Returns the groups the account belongs to and computed permission list.")]
    async fn get_account_permissions(
        &self,
        Parameters(params): Parameters<GetAccountPermissionsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let groups = ds.get_account_groups(&params.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let perms = ds.load_account_permissions(&params.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let group_items: Vec<serde_json::Value> = groups.into_iter().map(|g| {
            serde_json::json!({"id": g.id, "name": g.name, "domain": g.domain})
        }).collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "address": params.address,
                "groups": group_items,
                "permissions": perms.permission_list(),
                "accessible_domains": perms.accessible_domains(),
                "is_super": perms.is_super(),
            }).to_string(),
        )]))
    }

    #[tool(description = "Add an account to a permission group. Use list_groups to find group IDs. Requires admin.groups permission.")]
    async fn add_account_to_group(
        &self,
        Parameters(params): Parameters<AddAccountToGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        ds.add_account_to_group(&params.address, params.group_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "added", "address": params.address, "group_id": params.group_id}).to_string(),
        )]))
    }

    #[tool(description = "Remove an account from a permission group. Requires admin.groups permission.")]
    async fn remove_account_from_group(
        &self,
        Parameters(params): Parameters<RemoveAccountFromGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.groups")?;
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let removed = ds.remove_account_from_group(&params.address, params.group_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        if removed {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({"status": "removed", "address": params.address, "group_id": params.group_id}).to_string(),
            )]))
        } else {
            Err(McpError::invalid_params("membership not found", None))
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
                "mailrs MCP server — tools for email operations (send, read, search, reply) and account/permission management (create/remove accounts, manage group memberships). Admin tools require appropriate permissions.",
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
                    permissions: std::sync::Arc::new(
                        crate::permission::compute_effective_permissions(&[], &[], &[]),
                    ),
                    auth_method: AuthMethod::Session,
                });
            Ok(MailMcpService::new(state_clone.clone(), auth_user))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    axum::Router::new().nest_service("/mcp", service)
}
