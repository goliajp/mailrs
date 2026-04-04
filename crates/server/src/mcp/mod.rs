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

use self::tools::*;

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

        // resolve attachments from base64 / URL / file path
        let attachment_data = resolve_attachments(params.attachments.unwrap_or_default()).await?;

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
            false,
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
                None,
                None,
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

    // --- shared helpers ---

    fn ds(&self) -> Result<&Arc<crate::domain_store::DomainStore>, McpError> {
        self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))
    }

    fn pool(&self) -> Result<&sqlx::PgPool, McpError> {
        self.web_state.pg_pool.as_ref()
            .ok_or_else(|| McpError::internal_error("database unavailable", None))
    }

    fn mb_store(&self) -> Result<&Arc<mailrs_mailbox::MailboxStore>, McpError> {
        self.web_state.mailbox_store.as_ref()
            .ok_or_else(|| McpError::internal_error("mailbox store not available", None))
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
        let domain = target_user
            .split_once('@')
            .map(|(_, d)| d)
            .unwrap_or("");
        if domain.is_empty() {
            return Err(McpError::invalid_params("invalid target user address", None));
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
        if let Err(e) = crate::users::validate_email(&params.address) {
            return Err(McpError::invalid_params(e, None));
        }
        let ds = self.web_state.domain_store.as_ref()
            .ok_or_else(|| McpError::internal_error("domain store not available", None))?;

        let password_hash = if params.password.is_empty() {
            String::new()
        } else {
            if let Err(e) = crate::users::validate_password(&params.password) {
                return Err(McpError::invalid_params(e, None));
            }
            crate::users::UserStore::hash_password(&params.password)
                .map_err(|_| McpError::internal_error("failed to hash password", None))?
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

    // --- domain management ---

    #[tool(description = "List all managed domains. Returns domain name and creation date.")]
    async fn list_domains(
        &self,
        Parameters(_params): Parameters<ListDomainsParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let domains = ds.list_domains().await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = domains.into_iter()
            .map(|d| serde_json::json!({"name": d.name, "created_at": d.created_at}))
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Add a new domain. Requires admin.domains permission.")]
    async fn add_domain(
        &self,
        Parameters(params): Parameters<AddDomainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.domains")?;
        let ds = self.ds()?;
        ds.add_domain(&params.name, chrono::Utc::now().timestamp()).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("domain_added", &params.name)
    }

    #[tool(description = "Remove a domain and all its accounts/aliases. Requires admin.domains permission.")]
    async fn remove_domain(
        &self,
        Parameters(params): Parameters<RemoveDomainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.domains")?;
        let ds = self.ds()?;
        let removed = ds.remove_domain(&params.name).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("domain_removed", &params.name)
        } else {
            Err(McpError::invalid_params("domain not found", None))
        }
    }

    // --- alias management ---

    #[tool(description = "List all email aliases and forwards. Returns source, target, domain, type.")]
    async fn list_aliases(
        &self,
        Parameters(_params): Parameters<ListAliasesParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let aliases = ds.list_aliases().await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = aliases.into_iter()
            .map(|a| serde_json::json!({
                "id": a.id, "source_address": a.source_address,
                "target_address": a.target_address, "domain": a.domain,
                "alias_type": a.alias_type, "active": a.active,
            }))
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Add an email alias or forward. Type 'alias' delivers to local account, 'forward' relays externally. Requires admin.aliases permission.")]
    async fn add_alias(
        &self,
        Parameters(params): Parameters<AddAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.aliases")?;
        let ds = self.ds()?;
        let id = ds.add_alias(
            &params.source_address, &params.target_address,
            &params.domain, &params.alias_type, 0,
        ).await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "alias_added", "id": id, "source": params.source_address, "target": params.target_address}).to_string(),
        )]))
    }

    #[tool(description = "Remove an email alias by ID. Requires admin.aliases permission.")]
    async fn remove_alias(
        &self,
        Parameters(params): Parameters<RemoveAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.aliases")?;
        let ds = self.ds()?;
        let removed = ds.remove_alias(params.id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("alias_removed", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("alias not found", None))
        }
    }

    // --- app management ---

    #[tool(description = "List all registered apps with their scopes and status.")]
    async fn list_apps(
        &self,
        Parameters(_params): Parameters<ListAppsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let apps = ds.list_apps(None).await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = apps.into_iter()
            .map(|a| serde_json::json!({
                "app_id": a.app_id, "name": a.name, "description": a.description,
                "scopes": a.scopes, "owner": a.owner_address, "active": a.active,
            }))
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Register a new app and generate its API key. The key is only returned once. Requires admin.accounts permission.")]
    async fn create_app(
        &self,
        Parameters(params): Parameters<CreateAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let pool = self.web_state.pg_pool.as_ref()
            .ok_or_else(|| McpError::internal_error("database unavailable", None))?;

        let app_id = uuid::Uuid::new_v4().to_string();
        let id = ds.create_app(&app_id, &params.name, &params.description, &self.auth_user.address, &params.scopes).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let (full_key, prefix, key_hash) = crate::api_key_store::generate_api_key();
        let key_id = crate::api_key_store::insert_app_api_key(
            pool, &prefix, &key_hash, &full_key, &self.auth_user.address, &params.name, id, None,
        ).await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "app_id": app_id, "name": params.name, "scopes": params.scopes,
                "api_key": {"id": key_id, "key": full_key, "prefix": prefix},
                "warning": "Save this API key now. It cannot be retrieved again.",
            }).to_string(),
        )]))
    }

    #[tool(description = "Delete an app and revoke all its API keys. Requires admin.accounts permission.")]
    async fn delete_app(
        &self,
        Parameters(params): Parameters<DeleteAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let removed = ds.remove_app(&params.app_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("app_deleted", &params.app_id)
        } else {
            Err(McpError::invalid_params("app not found", None))
        }
    }

    // --- webhook management ---

    #[tool(description = "List your webhook subscriptions.")]
    async fn list_webhooks(
        &self,
        Parameters(_params): Parameters<ListWebhooksParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let subs = crate::webhook::store::list_subscriptions(pool, &self.auth_user.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = subs.into_iter()
            .map(|s| serde_json::json!({
                "id": s.id, "url": s.url, "event_type": s.event_type,
                "filter_sender": s.filter_sender, "filter_thread_id": s.filter_thread_id,
                "active": s.active,
            }))
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Create a webhook subscription for new email events. Returns the signing secret (save it, shown only once).")]
    async fn create_webhook(
        &self,
        Parameters(params): Parameters<CreateWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let signing_secret = crate::webhook::store::generate_signing_secret();
        let id = crate::webhook::store::create_subscription(
            pool, &self.auth_user.address, &params.url, &params.event_type,
            params.filter_sender.as_deref(), params.filter_thread_id.as_deref(),
            &signing_secret,
        ).await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "id": id, "url": params.url, "event_type": params.event_type,
                "signing_secret": signing_secret,
                "warning": "Save this signing secret now. It cannot be retrieved again.",
            }).to_string(),
        )]))
    }

    #[tool(description = "Delete a webhook subscription by ID.")]
    async fn delete_webhook(
        &self,
        Parameters(params): Parameters<DeleteWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let removed = crate::webhook::store::delete_subscription(pool, params.id, &self.auth_user.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("webhook_deleted", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("webhook not found", None))
        }
    }

    // --- mail operations ---

    #[tool(description = "List mailbox folders with message counts (total, unseen).")]
    async fn get_folders(
        &self,
        Parameters(_params): Parameters<GetFoldersParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let _ = mb_store.ensure_default_mailboxes(&self.auth_user.address).await;
        let mailboxes = mb_store.list_mailboxes(&self.auth_user.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let mut items = Vec::with_capacity(mailboxes.len());
        for mb in &mailboxes {
            let (total, unseen) = mb_store.mailbox_status(mb.id).await.unwrap_or((0, 0));
            items.push(serde_json::json!({"name": mb.name, "total": total, "unseen": unseen}));
        }
        self.json_result(&items)
    }

    #[tool(description = "Mark all messages in a thread as read.")]
    async fn mark_thread_read(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.mark_thread_read(&self.auth_user.address, &params.thread_id, None).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("marked_read", &params.thread_id)
    }

    #[tool(description = "Mark a thread as unread.")]
    async fn mark_thread_unread(
        &self,
        Parameters(params): Parameters<MarkThreadUnreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.mark_thread_unread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("marked_unread", &params.thread_id)
    }

    #[tool(description = "Star/flag a thread for importance.")]
    async fn star_thread(
        &self,
        Parameters(params): Parameters<StarThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.star_thread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("starred", &params.thread_id)
    }

    #[tool(description = "Remove star/flag from a thread.")]
    async fn unstar_thread(
        &self,
        Parameters(params): Parameters<UnstarThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.unstar_thread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("unstarred", &params.thread_id)
    }

    #[tool(description = "Archive a thread (hide from inbox).")]
    async fn archive_thread(
        &self,
        Parameters(params): Parameters<ArchiveThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.archive_thread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("archived", &params.thread_id)
    }

    #[tool(description = "Unarchive a thread (restore to inbox).")]
    async fn unarchive_thread(
        &self,
        Parameters(params): Parameters<UnarchiveThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.unarchive_thread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("unarchived", &params.thread_id)
    }

    #[tool(description = "Delete a thread and all its messages permanently.")]
    async fn delete_thread(
        &self,
        Parameters(params): Parameters<DeleteThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store.delete_thread(&self.auth_user.address, &params.thread_id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("deleted", &params.thread_id)
    }

    #[tool(description = "Get email category counts (personal, notification, promotion, etc.).")]
    async fn get_categories(
        &self,
        Parameters(_params): Parameters<GetCategoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let cats = mb_store.list_conversation_categories(&self.auth_user.address, None).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = cats.into_iter()
            .map(|(cat, count)| serde_json::json!({"category": cat, "count": count}))
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Search contacts from email history. Returns address, display name, counts.")]
    async fn get_contacts(
        &self,
        Parameters(_params): Parameters<GetContactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let contacts = mb_store.search_contacts(&self.auth_user.address, "", 100).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = contacts.into_iter()
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .collect();
        self.json_result(&items)
    }

    // --- queue management ---

    #[tool(description = "List outbound delivery queue entries. Requires admin.queue permission.")]
    async fn get_queue(
        &self,
        Parameters(_params): Parameters<GetQueueParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self.web_state.outbound_queue.as_ref()
            .ok_or_else(|| McpError::internal_error("outbound queue not configured", None))?;
        let entries = mailrs_outbound_queue::queue::list_recent(pool, 100).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = entries.into_iter()
            .map(|m| serde_json::json!({
                "id": m.id, "sender": m.sender, "recipient": m.recipient,
                "domain": m.domain, "status": m.status.as_str(),
                "attempts": m.attempts, "last_error": m.last_error,
            }))
            .collect();
        self.json_result(&items)
    }

    // --- email group management ---

    #[tool(description = "List all email groups (distribution lists). Returns group address, domain, name, member count.")]
    async fn list_email_groups(
        &self,
        Parameters(_params): Parameters<ListEmailGroupsParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let groups = ds.list_email_groups(None).await.map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let mut items = Vec::with_capacity(groups.len());
        for g in &groups {
            let members = ds.list_email_group_members(g.id).await.unwrap_or_default();
            items.push(serde_json::json!({
                "id": g.id, "address": g.address, "domain": g.domain,
                "name": g.name, "description": g.description,
                "member_count": members.len(), "members": members,
            }));
        }
        self.json_result(&items)
    }

    #[tool(description = "Create an email group (distribution list). All members receive copies of incoming mail and can reply as the group address. Requires admin.accounts permission.")]
    async fn create_email_group(
        &self,
        Parameters(params): Parameters<CreateEmailGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let id = ds.create_email_group(&params.address, &params.domain, &params.name, &params.description).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"status": "created", "id": id, "address": params.address}).to_string(),
        )]))
    }

    #[tool(description = "Delete an email group by ID. Requires admin.accounts permission.")]
    async fn delete_email_group(
        &self,
        Parameters(params): Parameters<DeleteEmailGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        match ds.remove_email_group(params.id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?
        {
            Some(addr) => self.ok_result("deleted", &addr),
            None => Err(McpError::invalid_params("email group not found", None)),
        }
    }

    #[tool(description = "List members of an email group.")]
    async fn list_email_group_members(
        &self,
        Parameters(params): Parameters<ListEmailGroupMembersParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let members = ds.list_email_group_members(params.id).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = members.into_iter().map(|m| serde_json::json!(m)).collect();
        self.json_result(&items)
    }

    #[tool(description = "Add a member to an email group. The member will receive group emails and can reply as the group address. Requires admin.accounts permission.")]
    async fn add_email_group_member(
        &self,
        Parameters(params): Parameters<AddEmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        ds.add_email_group_member(params.group_id, &params.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("member_added", &params.address)
    }

    #[tool(description = "Remove a member from an email group. Requires admin.accounts permission.")]
    async fn remove_email_group_member(
        &self,
        Parameters(params): Parameters<RemoveEmailGroupMemberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let removed = ds.remove_email_group_member(params.group_id, &params.address).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("member_removed", &params.address)
        } else {
            Err(McpError::invalid_params("member not found", None))
        }
    }

    #[tool(description = "Schedule an email to be sent at a future time. Returns message ID on success.")]
    async fn send_scheduled_email(
        &self,
        Parameters(params): Parameters<SendScheduledEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.to.is_empty() {
            return Err(McpError::invalid_params("recipient list is empty", None));
        }
        if params.to.len() > 50 {
            return Err(McpError::invalid_params("too many recipients (max 50)", None));
        }

        let scheduled_ts = chrono::DateTime::parse_from_rfc3339(&params.scheduled_at)
            .map_err(|e| McpError::invalid_params(format!("invalid scheduled_at: {e}"), None))?
            .timestamp();

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

        let raw = crate::web::mail::build_rfc5322_with_attachments(
            from,
            &params.to,
            &[],
            &params.subject,
            &params.body,
            None,
            &message_id,
            None,
            &[],
            &now,
            &[],
            None,
            &[],
            false,
        );

        let result = crate::web::mail::deliver_message_ex(
            &self.web_state,
            from,
            &params.to,
            &[],
            &[],
            &raw,
            &message_id,
            now.timestamp(),
            Some(scheduled_ts),
        )
        .await;

        let body = result.0;
        if body.success {
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({
                    "message_id": message_id,
                    "status": "scheduled",
                    "scheduled_at": params.scheduled_at,
                })
                .to_string(),
            )]))
        } else {
            Err(McpError::internal_error(
                body.message.unwrap_or_else(|| "scheduling failed".to_string()),
                None,
            ))
        }
    }

    #[tool(description = "Query recent audit log entries. Requires admin.accounts permission. Returns actor, action, target, detail, and timestamp.")]
    async fn get_audit_log(
        &self,
        Parameters(params): Parameters<GetAuditLogParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let limit = params.limit.min(200) as i64;
        let entries = ds.list_audit_log(limit).await
            .map_err(|e| McpError::internal_error(format!("failed to query audit log: {e}"), None))?;

        let items: Vec<serde_json::Value> = entries.into_iter().map(|e| {
            serde_json::json!({
                "id": e.id,
                "timestamp": e.timestamp,
                "actor": e.actor,
                "action": e.action,
                "target": e.target,
                "detail": e.detail,
            })
        }).collect();

        self.json_result(&items)
    }

    #[tool(description = "Retry a failed outbound message. Requires admin.queue permission.")]
    async fn retry_queue_message(
        &self,
        Parameters(params): Parameters<RetryQueueMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self.web_state.outbound_queue.as_ref()
            .ok_or_else(|| McpError::internal_error("outbound queue not configured", None))?;
        let now = chrono::Utc::now().timestamp();
        let retried = mailrs_outbound_queue::queue::retry_message(pool, params.id, now).await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if retried {
            self.ok_result("retrying", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("message not found or not retryable", None))
        }
    }
    // --- signature management (user-level, no admin required) ---

    #[tool(description = "List your email signatures. Returns id, name, html, text_content, is_default, and created_at.")]
    async fn list_signatures(
        &self,
        Parameters(_params): Parameters<ListSignaturesParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let rows = sqlx::query_as::<_, (i64, String, String, String, bool, String)>(
            "SELECT id, name, html, text_content, is_default, \
             to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
             FROM signatures WHERE account_address = $1 ORDER BY created_at",
        )
        .bind(&self.auth_user.address)
        .fetch_all(pool)
        .await
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let items: Vec<serde_json::Value> = rows.into_iter().map(|(id, name, html, text_content, is_default, created_at)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "html": html,
                "text_content": text_content,
                "is_default": is_default,
                "created_at": created_at,
            })
        }).collect();

        self.json_result(&items)
    }

    #[tool(description = "Create or update an email signature. Provide id to update an existing signature, omit to create new.")]
    async fn save_signature(
        &self,
        Parameters(params): Parameters<SaveSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;

        if params.name.len() > 200 {
            return Err(McpError::invalid_params("signature name too long", None));
        }
        if params.html.len() > 100_000 || params.text_content.len() > 100_000 {
            return Err(McpError::invalid_params("signature content too long", None));
        }

        // if setting as default, unset any existing default first
        if params.is_default {
            let _ = sqlx::query(
                "UPDATE signatures SET is_default = false WHERE account_address = $1",
            )
            .bind(&self.auth_user.address)
            .execute(pool)
            .await;
        }

        let result = if let Some(id) = params.id {
            sqlx::query(
                "UPDATE signatures SET name = $1, html = $2, text_content = $3, is_default = $4 \
                 WHERE id = $5 AND account_address = $6",
            )
            .bind(&params.name)
            .bind(&params.html)
            .bind(&params.text_content)
            .bind(params.is_default)
            .bind(id)
            .bind(&self.auth_user.address)
            .execute(pool)
            .await
        } else {
            sqlx::query(
                "INSERT INTO signatures (account_address, name, html, text_content, is_default) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&self.auth_user.address)
            .bind(&params.name)
            .bind(&params.html)
            .bind(&params.text_content)
            .bind(params.is_default)
            .execute(pool)
            .await
        };

        result
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        self.ok_result("saved", &params.name)
    }

    #[tool(description = "Delete an email signature by ID. Only deletes signatures owned by the authenticated user.")]
    async fn delete_signature(
        &self,
        Parameters(params): Parameters<DeleteSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let result = sqlx::query(
            "DELETE FROM signatures WHERE id = $1 AND account_address = $2",
        )
        .bind(params.id)
        .bind(&self.auth_user.address)
        .execute(pool)
        .await
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        if result.rows_affected() > 0 {
            self.ok_result("deleted", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("signature not found", None))
        }
    }

    // --- encryption key management ---

    #[tool(description = "List your encryption keys (PGP and S/MIME). Returns key type, fingerprint, and creation time — not the raw key data.")]
    async fn list_encryption_keys(
        &self,
        Parameters(_params): Parameters<ListEncryptionKeysParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let rows = ds
            .list_encryption_keys(&self.auth_user.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, key_type, fingerprint, created_at)| {
                serde_json::json!({
                    "id": id,
                    "key_type": key_type,
                    "fingerprint": fingerprint,
                    "created_at": created_at,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(description = "Upload or replace your PGP public key or S/MIME certificate. Key type must be 'pgp' or 'smime'.")]
    async fn set_encryption_key(
        &self,
        Parameters(params): Parameters<SetEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        if params.public_key.is_empty() {
            return Err(McpError::invalid_params("public_key is required", None));
        }
        if params.public_key.len() > 256 * 1024 {
            return Err(McpError::invalid_params("public_key too large", None));
        }
        let ds = self.ds()?;
        let id = ds
            .set_encryption_key(
                &self.auth_user.address,
                &params.key_type,
                &params.public_key,
                &params.fingerprint,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "status": "key_saved",
                "id": id,
                "key_type": params.key_type,
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Delete your PGP public key or S/MIME certificate. Key type must be 'pgp' or 'smime'.")]
    async fn delete_encryption_key(
        &self,
        Parameters(params): Parameters<DeleteEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        let ds = self.ds()?;
        let removed = ds
            .delete_encryption_key(&self.auth_user.address, &params.key_type)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("key_deleted", &params.key_type)
        } else {
            Err(McpError::invalid_params("key not found", None))
        }
    }

    #[tool(description = "Look up a recipient's PGP public key or S/MIME certificate by email address. Use this before encrypting an email to someone.")]
    async fn get_recipient_key(
        &self,
        Parameters(params): Parameters<GetRecipientKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        if !params.address.contains('@') {
            return Err(McpError::invalid_params("invalid email address", None));
        }
        let ds = self.ds()?;
        match ds
            .get_encryption_key(&params.address, &params.key_type)
            .await
        {
            Ok(Some((_id, public_key, fingerprint))) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::json!({
                        "address": params.address,
                        "key_type": params.key_type,
                        "public_key": public_key,
                        "fingerprint": fingerprint,
                    })
                    .to_string(),
                )]))
            }
            Ok(None) => Err(McpError::invalid_params(
                format!("no {} key found for {}", params.key_type, params.address),
                None,
            )),
            Err(e) => Err(McpError::internal_error(format!("{e}"), None)),
        }
    }

    // --- mail audit ---

    #[tool(description = "List email conversations for a target user (audit/compliance). Requires admin.impersonate permission. Target user must be in your accessible domains.")]
    async fn audit_list_conversations(
        &self,
        Parameters(params): Parameters<AuditListConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.impersonate")?;
        self.validate_audit_target(&params.target_user)?;

        let mb = self.mb_store()?;
        let limit = params.limit.unwrap_or(20).min(50);
        let convos = mb
            .list_conversations(
                &params.target_user,
                limit,
                None,
                params.category.as_deref(),
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        // audit log
        if let Ok(ds) = self.ds() {
            ds.log_audit(
                &self.auth_user.address,
                "audit.list_conversations",
                &params.target_user,
                "",
            )
            .await;
        }

        let items: Vec<serde_json::Value> = convos
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "participants": c.participants,
                    "message_count": c.message_count,
                    "last_date": c.last_date,
                    "category": c.category,
                    "snippet": c.snippet,
                })
            })
            .collect();

        self.json_result(&items)
    }

    #[tool(description = "Read all messages in a thread for a target user (audit/compliance). Requires admin.impersonate permission. Target user must be in your accessible domains.")]
    async fn audit_read_thread(
        &self,
        Parameters(params): Parameters<AuditReadThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.impersonate")?;
        self.validate_audit_target(&params.target_user)?;

        let mb = self.mb_store()?;
        let messages = mb
            .list_thread_messages(&params.target_user, &params.thread_id, None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        // audit log
        if let Ok(ds) = self.ds() {
            ds.log_audit(
                &self.auth_user.address,
                "audit.read_thread",
                &params.target_user,
                &params.thread_id,
            )
            .await;
        }

        let items: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                let maildir_user = if msg.user_address.is_empty() {
                    &params.target_user
                } else {
                    &msg.user_address
                };
                let raw = crate::message_util::read_message_raw(
                    &self.web_state.maildir_root,
                    maildir_user,
                    &msg.maildir_id,
                );
                let parsed = raw
                    .as_deref()
                    .map(crate::message_util::parse_message)
                    .unwrap_or_default();

                serde_json::json!({
                    "id": msg.id,
                    "uid": msg.uid,
                    "sender": msg.sender,
                    "recipients": msg.recipients,
                    "subject": msg.subject,
                    "internal_date": msg.internal_date,
                    "text_body": parsed.0,
                    "attachments": parsed.2.iter().map(|a| serde_json::json!({"filename": a.filename, "content_type": a.content_type, "size": a.size})).collect::<Vec<_>>(),
                })
            })
            .collect();

        self.json_result(&items)
    }

    // --- system config tools ---

    #[tool(description = "Get all system configuration entries with current values, types, sources (database/env/default), and metadata. Requires admin.system_config permission.")]
    async fn get_system_config(
        &self,
        Parameters(_params): Parameters<GetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store = self
            .web_state
            .system_config
            .as_ref()
            .ok_or_else(|| McpError::internal_error("system config store not available", None))?;

        let entries = store.get_all_entries();
        let items: Vec<serde_json::Value> = entries
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "value_type": e.value_type,
                    "group": e.group,
                    "description": e.description,
                    "source": e.source,
                    "updated_at": e.updated_at,
                    "updated_by": e.updated_by,
                })
            })
            .collect();

        self.json_result(&items)
    }

    #[tool(description = "Set a system configuration value. Validates key and value type. Requires admin.system_config permission. Use get_system_config to see available keys.")]
    async fn set_system_config(
        &self,
        Parameters(params): Parameters<SetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store = self
            .web_state
            .system_config
            .as_ref()
            .ok_or_else(|| McpError::internal_error("system config store not available", None))?;

        store
            .set(&params.key, &params.value, &self.auth_user.address)
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;

        self.ok_result("updated", &format!("{} = {}", params.key, params.value))
    }

    #[tool(description = "Reset a system configuration key to its default value (removes database override). Requires admin.system_config permission.")]
    async fn reset_system_config(
        &self,
        Parameters(params): Parameters<ResetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store = self
            .web_state
            .system_config
            .as_ref()
            .ok_or_else(|| McpError::internal_error("system config store not available", None))?;

        store
            .delete(&params.key)
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;

        self.ok_result("reset", &format!("{} reverted to default", params.key))
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

const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024; // 25 MB

/// resolve attachment data field: URL → download, existing file → read, otherwise → base64 decode
async fn resolve_attachments(
    attachments: Vec<tools::Attachment>,
) -> Result<Vec<crate::web::mail::AttachmentData>, McpError> {
    let mut results = Vec::with_capacity(attachments.len());

    for a in attachments {
        let data_str = a.data.trim();

        // determine source type
        let (bytes, derived_filename) =
            if data_str.starts_with("http://") || data_str.starts_with("https://") {
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
                        format!(
                            "download failed for '{}': HTTP {}",
                            data_str,
                            resp.status()
                        ),
                        None,
                    ));
                }
                let bytes = resp.bytes().await.map_err(|e| {
                    McpError::invalid_params(
                        format!("failed to read response body: {e}"),
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
                // derive filename from URL path
                let name = url_filename(data_str);
                (bytes.to_vec(), name)
            } else if tokio::fs::metadata(data_str).await.is_ok() {
                // file path: read from disk
                let bytes = tokio::fs::read(data_str).await.map_err(|e| {
                    McpError::invalid_params(
                        format!("failed to read file '{}': {e}", data_str),
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
                            "attachment data is not a valid URL, file path, or base64 string".to_string(),
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
