//! The v1 MCP read tools.
//!
//! Second half of the `mcp/mod.rs` split (2026-08-02): with these out,
//! `mod.rs` holds the service type, the `ServerHandler` description, the
//! `/mcp` mount and the auth middleware, and no tools at all.
//!
//! Tool **names** are the wire contract and must match the monolith lane —
//! `scripts/check-mcp-parity.sh` holds both to the same 82.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::ok_result;
use super::params::*;

#[tool_router(router = tool_router_v1_read, vis = "pub")]
impl MailrsMcpService {
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
        description = "Send an email. Enqueues to the outbound queue; delivery is asynchronous. Pass `scheduled_at` (Unix epoch seconds, future) to schedule instead of sending now. Returns the assigned Message-ID."
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
            params.scheduled_at,
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

    #[tool(description = "Search the authenticated user's contacts (autocomplete addresses).")]
    async fn search_contacts(
        &self,
        Parameters(params): Parameters<SearchContactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let key = format!("mailrs:user:{user}:contacts");
        let q = params.q.to_lowercase();
        let limit = params.limit as usize;
        let flat = crate::handlers::kevy_util::with_kevy(move |c| {
            c.hgetall(key.as_bytes()).map_err(std::io::Error::other)
        })
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
        let flat = crate::handlers::kevy_util::with_kevy(move |c| {
            c.hgetall(key.as_bytes()).map_err(std::io::Error::other)
        })
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
        let pending = crate::handlers::kevy_util::with_kevy(|c| {
            c.llen(b"mailrs:outbound:pending-idx")
                .map_err(std::io::Error::other)
        })
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
}
