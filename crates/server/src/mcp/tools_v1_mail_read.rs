//! v1 mail tools that only read.
//!
//! Second half of the `tools_v1_mail` split (2026-08-02).
use super::MailMcpService;
use super::tools::*;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

#[tool_router(router = tool_router_v1_mail_read, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "Read every message in a thread. Returns sender, recipients, subject, date, and decoded text body per message, oldest first."
    )]
    async fn read_thread(
        &self,
        Parameters(params): Parameters<ReadThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(ref mb_store) = self.web_state.mailbox_store else {
            return Err(McpError::internal_error(
                "mailbox store not available",
                None,
            ));
        };
        let user = &self.auth_user.address;
        let metas = mb_store
            .list_thread_messages(user, &params.thread_id, None)
            .await
            .map_err(|e| McpError::internal_error(format!("list_thread_messages: {e}"), None))?;
        let mut items = Vec::with_capacity(metas.len());
        for msg in &metas {
            let raw = crate::message_util::read_message_raw(
                &self.web_state.maildir_root,
                user,
                &msg.maildir_id,
            )
            .await;
            let (text_body, _html_body, attachments) = raw
                .as_deref()
                .map(crate::message_util::parse_message)
                .unwrap_or_default();
            items.push(serde_json::json!({
                "uid": msg.uid,
                "sender": crate::message_util::decode_header(&msg.sender),
                "recipients": crate::message_util::decode_header(&msg.recipients),
                "subject": crate::message_util::decode_header(&msg.subject),
                "internal_date": msg.internal_date,
                "text_body": text_body.unwrap_or_default(),
                "attachments": attachments,
                "message_id": msg.message_id,
            }));
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "thread_id": params.thread_id, "messages": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Search emails by keyword. Returns conversation summaries (thread_id, subject, snippet, participants)."
    )]
    async fn search_conversations(
        &self,
        Parameters(params): Parameters<SearchConversationsParams>,
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
            .search_conversations(user, &params.q, limit, None, None)
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

    #[tool(
        description = "List recent email conversations. Returns thread summaries (thread_id, subject, snippet, participants, unread count)."
    )]
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

    #[tool(description = "List mailbox folders with message counts (total, unseen).")]
    async fn list_mailboxes(
        &self,
        Parameters(_params): Parameters<ListMailboxesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let _ = mb_store
            .ensure_default_mailboxes(&self.auth_user.address)
            .await;
        let mailboxes = mb_store
            .list_mailboxes(&self.auth_user.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let mut items = Vec::with_capacity(mailboxes.len());
        for mb in &mailboxes {
            let (total, unseen) = mb_store.mailbox_status(mb.id).await.unwrap_or((0, 0));
            items.push(serde_json::json!({"name": mb.name, "total": total, "unseen": unseen}));
        }
        self.json_result(&items)
    }

    #[tool(description = "Get email category counts (personal, notification, promotion, etc.).")]
    async fn get_categories(
        &self,
        Parameters(_params): Parameters<GetCategoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let cats = mb_store
            .list_conversation_categories(&self.auth_user.address, None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = cats
            .into_iter()
            .map(|(cat, count)| serde_json::json!({"category": cat, "count": count}))
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Search contacts from email history. Returns address, display name, counts."
    )]
    async fn search_contacts(
        &self,
        Parameters(_params): Parameters<SearchContactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let contacts = mb_store
            .search_contacts(&self.auth_user.address, "", 100)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = contacts
            .into_iter()
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "List your email signatures. Returns id, name, html, text_content, is_default, and created_at."
    )]
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

        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, name, html, text_content, is_default, created_at)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "html": html,
                    "text_content": text_content,
                    "is_default": is_default,
                    "created_at": created_at,
                })
            })
            .collect();

        self.json_result(&items)
    }
}
