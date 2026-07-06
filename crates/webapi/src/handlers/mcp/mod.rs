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
//! - `mark_thread_unread` / `star_thread` / `unstar_thread`
//! - `archive_thread` / `unarchive_thread` / `delete_thread`
//! - `mark_all_read` — zero every unread counter
//! - `get_categories` — inbox category histogram
//!
//! - `search_contacts` / `list_signatures` / `get_queue`
//! - `list_accounts` / `list_domains` (admin-gated)
//!
//! - `create_account` / `remove_account` / `get_audit_log` (admin-gated)
//! - `add_alias` / `remove_alias` / `add_domain` / `remove_domain` (admin-gated)
//! - `save_signature` / `delete_signature` (per-user, no admin)
//! - `list_webhooks` / `create_webhook` / `delete_webhook`
//! - `list_drafts` / `save_draft` / `delete_draft` (per-user)
//! - `list_templates` / `save_template` / `delete_template` (per-user)
//!
//! 37 tools (was 6). Remaining monolith tools (groups, permissions,
//! encryption, email-groups, greylist, apps, system-config) fill in
//! as follow-on G11 batches.
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

use crate::WebState;

mod params;
use params::*;

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

#[tool_router]
impl MailrsMcpService {
    pub fn new(state: Arc<WebState>, user: String) -> Self {
        Self {
            state,
            user,
            tool_router: Self::tool_router(),
        }
    }

    /// Gate an admin tool: the authed user must carry an admin.*
    /// permission (or be super). Maps a FORBIDDEN to an MCP error.
    async fn require_admin(&self, user: &str) -> Result<(), McpError> {
        crate::handlers::kevy_util::require_admin(&self.state, user)
            .await
            .map_err(|_| McpError::invalid_request("admin permission required", None))
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
            .core
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
            .core
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
            .core
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
            .core
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
            .core
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

    #[tool(description = "Mark a thread as unread (restore its unread counter).")]
    async fn mark_thread_unread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .mark_thread_unread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("mark_thread_unread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Star (flag) a thread.")]
    async fn star_thread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .star_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("star_thread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Remove the star (flag) from a thread.")]
    async fn unstar_thread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .unstar_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unstar_thread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Archive a thread (remove it from the inbox view).")]
    async fn archive_thread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .archive_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("archive_thread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Move an archived thread back into the inbox.")]
    async fn unarchive_thread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .unarchive_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unarchive_thread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Delete a thread (moves it out of every folder view).")]
    async fn delete_thread(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .delete_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_thread: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(description = "Mark every conversation as read in one call.")]
    async fn mark_all_read(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .mark_all_conversations_read(user)
            .await
            .map_err(|e| McpError::internal_error(format!("mark_all_read: {e}"), None))?;
        Ok(ok_result())
    }

    #[tool(
        description = "Category histogram for the inbox (personal / bulk / spam / ... with thread counts)."
    )]
    async fn get_categories(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let resp = self
            .state
            .core
            .conversation_categories(user)
            .await
            .map_err(|e| McpError::internal_error(format!("get_categories: {e}"), None))?;
        let items: Vec<_> = resp
            .categories
            .into_iter()
            .map(|c| serde_json::json!({ "category": c.category, "count": c.count }))
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "categories": items }).to_string(),
        )]))
    }

    // NOTE: contacts / signatures / outbound-queue / domains live in the
    // INDEPENDENT network kevy (shared side-state), NOT the switchable
    // core — so these tools read it directly, never through `state.core`.
    // (v2 dual-mode: the core client is mail-store only.)

    #[tool(description = "Search the authenticated user's contacts (autocomplete addresses).")]
    async fn search_contacts(
        &self,
        Parameters(params): Parameters<SearchContactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let key = format!("mailrs:user:{user}:contacts");
        let q = params.q.to_lowercase();
        let limit = params.limit as usize;
        let flat = crate::handlers::kevy_util::with_kevy(move |c| c.hgetall(key.as_bytes()))
            .map_err(|_| McpError::internal_error("contacts read failed", None))?;
        // hgetall is flat [field, value, ...] — field = email, value = display
        let mut items: Vec<String> = Vec::new();
        for pair in flat.chunks(2) {
            let email = String::from_utf8_lossy(&pair[0]).into_owned();
            let display = pair
                .get(1)
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .unwrap_or_default();
            if email.to_lowercase().contains(&q) || display.to_lowercase().contains(&q) {
                items.push(email);
                if items.len() >= limit {
                    break;
                }
            }
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "contacts": items }).to_string(),
        )]))
    }

    #[tool(description = "List the authenticated user's saved signatures.")]
    async fn list_signatures(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let key = format!("signatures:{user}");
        let flat = crate::handlers::kevy_util::with_kevy(move |c| c.hgetall(key.as_bytes()))
            .map_err(|_| McpError::internal_error("signatures read failed", None))?;
        let items: Vec<serde_json::Value> = flat
            .chunks(2)
            .filter_map(|p| p.get(1))
            .filter_map(|v| serde_json::from_slice(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "signatures": items }).to_string(),
        )]))
    }

    #[tool(description = "Outbound queue stats (pending count).")]
    async fn get_queue(&self) -> Result<CallToolResult, McpError> {
        let _user = self.require_user()?;
        let pending = crate::handlers::kevy_util::with_kevy(|c| c.llen(b"mailrs:outbound:pending"))
            .map_err(|_| McpError::internal_error("queue read failed", None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "pending": pending }).to_string(),
        )]))
    }

    #[tool(description = "List all accounts (requires an admin permission).")]
    async fn list_accounts(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let resp = self
            .state
            .core
            .list_accounts()
            .await
            .map_err(|e| McpError::internal_error(format!("list_accounts: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "List all managed domains (requires an admin permission).")]
    async fn list_domains(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        // domains are shared side-state (env + network kevy admin:domains),
        // not switchable-core data — read network kevy directly.
        let flat = crate::handlers::kevy_util::with_kevy(|c| c.hgetall(b"admin:domains"))
            .map_err(|_| McpError::internal_error("domains read failed", None))?;
        let items: Vec<serde_json::Value> = flat
            .chunks(2)
            .filter_map(|p| p.get(1))
            .filter_map(|v| serde_json::from_slice(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "domains": items }).to_string(),
        )]))
    }

    #[tool(description = "Create a new account (requires an admin permission).")]
    async fn create_account(
        &self,
        Parameters(params): Parameters<CreateAccountParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let req = mailrs_core_api::method::admin::AddAccountRequest {
            address: params.address.clone(),
            display_name: params.display_name,
            password: params.password,
        };
        self.state
            .core
            .add_account(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("create_account: {e}"), None))?;
        crate::handlers::audit::record(&user, "account.create", &params.address, "via mcp");
        Ok(ok_result())
    }

    #[tool(description = "Remove an account (requires an admin permission).")]
    async fn remove_account(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_account(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_account: {e}"), None))?;
        crate::handlers::audit::record(&user, "account.delete", &params.address, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Add an email alias/forward (requires an admin permission). Type 'alias' delivers to a local account; 'forward' relays outbound."
    )]
    async fn add_alias(
        &self,
        Parameters(params): Parameters<AddAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let domain = params
            .source_address
            .rsplit_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_default();
        let req = mailrs_core_api::method::admin::AddAliasRequest {
            source_address: params.source_address.clone(),
            target_address: params.target_address.clone(),
            domain,
            alias_type: params.alias_type.unwrap_or_else(|| "alias".into()),
        };
        let resp = self
            .state
            .core
            .add_alias(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("add_alias: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "alias.create",
            &params.source_address,
            &format!("→ {}", params.target_address),
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Remove an alias by id (requires an admin permission).")]
    async fn remove_alias(
        &self,
        Parameters(params): Parameters<RemoveAliasParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_alias(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_alias: {e}"), None))?;
        crate::handlers::audit::record(&user, "alias.delete", &params.id.to_string(), "via mcp");
        Ok(ok_result())
    }

    #[tool(description = "Add a managed domain (requires an admin permission).")]
    async fn add_domain(
        &self,
        Parameters(params): Parameters<DomainNameParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .add_domain(&params.name)
            .await
            .map_err(|e| McpError::internal_error(format!("add_domain: {e}"), None))?;
        crate::handlers::audit::record(&user, "domain.create", &params.name, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Remove a managed domain and its dependent accounts/aliases (requires an admin permission)."
    )]
    async fn remove_domain(
        &self,
        Parameters(params): Parameters<DomainNameParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .remove_domain(&params.name)
            .await
            .map_err(|e| McpError::internal_error(format!("remove_domain: {e}"), None))?;
        crate::handlers::audit::record(&user, "domain.delete", &params.name, "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Save or update the caller's own email signature. Returns the new id — pass it to `delete_signature` to remove."
    )]
    async fn save_signature(
        &self,
        Parameters(params): Parameters<SaveSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveSignatureRequest {
            name: params.name.clone(),
            html: params.html,
            text_content: params.text_content,
            is_default: params.is_default,
        };
        let resp = self
            .state
            .core
            .save_signature(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_signature: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "signature.save",
            &format!("id={}", resp.id),
            &params.name,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own signatures by id.")]
    async fn delete_signature(
        &self,
        Parameters(params): Parameters<SignatureIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_signature(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_signature: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "signature.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(
        description = "List webhook subscriptions for an account (requires admin OR the caller owning the account)."
    )]
    async fn list_webhooks(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        if user != params.address {
            self.require_admin(&user).await?;
        }
        let resp = self
            .state
            .core
            .list_webhooks(&params.address)
            .await
            .map_err(|e| McpError::internal_error(format!("list_webhooks: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Create a webhook subscription. Returns id + signing_secret — store the secret, it isn't returned again."
    )]
    async fn create_webhook(
        &self,
        Parameters(params): Parameters<CreateWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        if user != params.account_address {
            self.require_admin(&user).await?;
        }
        let req = mailrs_core_api::method::admin::CreateWebhookRequest {
            account_address: params.account_address.clone(),
            url: params.url.clone(),
            event_type: params.event_type.clone(),
            filter_sender: params.filter_sender,
            filter_thread_id: params.filter_thread_id,
        };
        let resp = self
            .state
            .core
            .create_webhook(&req)
            .await
            .map_err(|e| McpError::internal_error(format!("create_webhook: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "webhook.create",
            &params.account_address,
            &format!("url={} event={}", params.url, params.event_type),
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete a webhook subscription by id (requires an admin permission).")]
    async fn delete_webhook(
        &self,
        Parameters(params): Parameters<WebhookIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        self.state
            .core
            .delete_webhook(params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_webhook: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "webhook.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "List the caller's own saved drafts.")]
    async fn list_drafts(
        &self,
        Parameters(params): Parameters<ListDraftsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let mut resp = self
            .state
            .core
            .list_drafts(&user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_drafts: {e}"), None))?;
        if let Some(limit) = params.limit {
            resp.items.truncate(limit as usize);
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Save a draft for the caller. Idempotent per body — returns the new id; overwrite by re-saving."
    )]
    async fn save_draft(
        &self,
        Parameters(params): Parameters<SaveDraftParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveDraftRequest {
            to: params.to,
            cc: params.cc,
            bcc: params.bcc,
            subject: params.subject.clone(),
            body: params.body,
            reply_to_thread_id: params.reply_to_thread_id,
        };
        let resp = self
            .state
            .core
            .save_draft(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_draft: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "draft.save",
            &format!("id={}", resp.id),
            &params.subject,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own drafts by id.")]
    async fn delete_draft(
        &self,
        Parameters(params): Parameters<DraftIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_draft(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_draft: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "draft.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "List the caller's own compose templates.")]
    async fn list_templates(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let resp = self
            .state
            .core
            .list_templates(&user)
            .await
            .map_err(|e| McpError::internal_error(format!("list_templates: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(
        description = "Save or update the caller's own compose template. Returns the new id — pass it to `delete_template` to remove."
    )]
    async fn save_template(
        &self,
        Parameters(params): Parameters<SaveTemplateParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = mailrs_core_api::method::admin::SaveTemplateRequest {
            name: params.name.clone(),
            subject: params.subject,
            html_body: params.html_body,
            text_body: params.text_body,
            category: params.category,
            is_default: params.is_default,
        };
        let resp = self
            .state
            .core
            .save_template(&user, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("save_template: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "template.save",
            &format!("id={}", resp.id),
            &params.name,
        );
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&resp).unwrap_or_default(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own compose templates by id.")]
    async fn delete_template(
        &self,
        Parameters(params): Parameters<TemplateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.state
            .core
            .delete_template(&user, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_template: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "template.delete",
            &format!("id={}", params.id),
            "via mcp",
        );
        Ok(ok_result())
    }

    #[tool(description = "Read the admin audit log, newest first (requires an admin permission).")]
    async fn get_audit_log(
        &self,
        Parameters(params): Parameters<AuditQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let limit = params.limit as i64;
        let rows = crate::handlers::kevy_util::with_kevy(move |c| {
            c.lrange(b"admin:audit_log", 0, limit - 1)
        })
        .map_err(|_| McpError::internal_error("audit read failed", None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .filter_map(|v| serde_json::from_slice(&v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "entries": items }).to_string(),
        )]))
    }
}

/// Shared `{ "ok": true }` success body for mutation tools.
fn ok_result() -> CallToolResult {
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
