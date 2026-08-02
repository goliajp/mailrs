//! v1 MCP tools that act on mail: threads, messages, sending, contacts.
//!
//! Split out of `mcp/mod.rs` on 2026-08-02, which held all sixty-one v1
//! tools in one 2,450-line file — the very thing
//! `.claude/rules/mcp-two-lane-parity.md` says not to keep doing. Tool
//! **names** are the wire contract and must match the fastcore lane;
//! `scripts/check-mcp-parity.sh` holds both to the same 82.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use rand_core::RngCore;

use super::MailMcpService;
use super::resolve_attachments;
use super::tools::*;

#[tool_router(router = tool_router_v1_mail, vis = "pub(crate)")]
impl MailMcpService {
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

        let from = params.from.as_deref().unwrap_or(&self.auth_user.address);

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
                body.message
                    .unwrap_or_else(|| "delivery failed".to_string()),
                None,
            ))
        }
    }

    #[tool(
        description = "Reply to an email thread. Automatically sets In-Reply-To headers. Returns message ID."
    )]
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

        let from = params.from.as_deref().unwrap_or(&self.auth_user.address);

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
            .map_err(|e| McpError::internal_error(format!("failed to load thread: {e}"), None))?;

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
                body.message
                    .unwrap_or_else(|| "delivery failed".to_string()),
                None,
            ))
        }
    }

    #[tool(description = "Mark all messages in a thread as read.")]
    async fn mark_thread_read(
        &self,
        Parameters(params): Parameters<MarkThreadReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .mark_thread_read(&self.auth_user.address, &params.thread_id, None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("marked_read", &params.thread_id)
    }

    #[tool(description = "Mark a thread as unread.")]
    async fn mark_thread_unread(
        &self,
        Parameters(params): Parameters<MarkThreadUnreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .mark_thread_unread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("marked_unread", &params.thread_id)
    }

    #[tool(description = "Star/flag a thread for importance.")]
    async fn star_thread(
        &self,
        Parameters(params): Parameters<StarThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .star_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("starred", &params.thread_id)
    }

    #[tool(description = "Remove star/flag from a thread.")]
    async fn unstar_thread(
        &self,
        Parameters(params): Parameters<UnstarThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .unstar_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("unstarred", &params.thread_id)
    }

    #[tool(description = "Archive a thread (hide from inbox).")]
    async fn archive_thread(
        &self,
        Parameters(params): Parameters<ArchiveThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .archive_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("archived", &params.thread_id)
    }

    #[tool(description = "Unarchive a thread (restore to inbox).")]
    async fn unarchive_thread(
        &self,
        Parameters(params): Parameters<UnarchiveThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .unarchive_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("unarchived", &params.thread_id)
    }

    #[tool(description = "Delete a thread and all its messages permanently.")]
    async fn delete_thread(
        &self,
        Parameters(params): Parameters<DeleteThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .delete_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        self.ok_result("deleted", &params.thread_id)
    }

    #[tool(
        description = "Schedule an email to be sent at a future time. Returns message ID on success."
    )]
    async fn send_scheduled_email(
        &self,
        Parameters(params): Parameters<SendScheduledEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.to.is_empty() {
            return Err(McpError::invalid_params("recipient list is empty", None));
        }
        if params.to.len() > 50 {
            return Err(McpError::invalid_params(
                "too many recipients (max 50)",
                None,
            ));
        }

        let scheduled_ts = chrono::DateTime::parse_from_rfc3339(&params.scheduled_at)
            .map_err(|e| McpError::invalid_params(format!("invalid scheduled_at: {e}"), None))?
            .timestamp();

        let from = params.from.as_deref().unwrap_or(&self.auth_user.address);

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
                body.message
                    .unwrap_or_else(|| "scheduling failed".to_string()),
                None,
            ))
        }
    }

    #[tool(
        description = "Create or update an email signature. Provide id to update an existing signature, omit to create new."
    )]
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
            let _ =
                sqlx::query("UPDATE signatures SET is_default = false WHERE account_address = $1")
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

        result.map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        self.ok_result("saved", &params.name)
    }

    #[tool(
        description = "Delete an email signature by ID. Only deletes signatures owned by the authenticated user."
    )]
    async fn delete_signature(
        &self,
        Parameters(params): Parameters<DeleteSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let result = sqlx::query("DELETE FROM signatures WHERE id = $1 AND account_address = $2")
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
}
