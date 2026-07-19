//! v2 MCP tool batch 14 — admin mail audit + the two convenience send
//! wrappers. Two-lane parity with the monolith's
//! `audit_list_conversations` / `audit_read_thread` / `reply_email` /
//! `send_scheduled_email`.
//!
//! The audit tools read another user's mail through the same core RPCs
//! `handlers::complete::{audit_conversations, audit_conversation_messages}`
//! use, and record an audit-log row per call.
//!
//! `reply_email` and `send_scheduled_email` are thin wrappers over the
//! same `prefs::send_email_mcp` path `send_email` drives — the fastcore
//! `send_email` already carries `in_reply_to` and `scheduled_at`, so
//! these only pre-fill them (reply resolves threading from the thread's
//! last message; scheduled makes the timestamp mandatory).

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::params::{
    AuditListConversationsParams, AuditReadThreadParams, ReplyEmailParams, SendScheduledEmailParams,
};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tool_router(router = tool_router_v2_batch14, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List a target user's conversations for audit / compliance. Admin-gated; every call is written to the audit log."
    )]
    async fn audit_list_conversations(
        &self,
        Parameters(params): Parameters<AuditListConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let limit = params.limit.unwrap_or(20).min(50);
        let req = mailrs_core_api::method::conversation::ListConversationsRequest {
            filter: mailrs_core_api::types::ConversationFilter {
                limit,
                ..Default::default()
            },
        };
        let resp = self
            .state
            .core
            .list_conversations(&params.target_user, &req)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("audit_list_conversations: {e}"), None)
            })?;
        crate::handlers::audit::record(
            &user,
            "audit.list_conversations",
            &params.target_user,
            "via mcp",
        );
        let items: Vec<_> = resp
            .items
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
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "target_user": params.target_user, "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Read every message in a target user's thread for audit / compliance, bodies included. Admin-gated; every call is written to the audit log."
    )]
    async fn audit_read_thread(
        &self,
        Parameters(params): Parameters<AuditReadThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let resp = self
            .state
            .core
            .list_thread_messages(&params.target_user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("audit_read_thread: {e}"), None))?;
        crate::handlers::audit::record(
            &user,
            "audit.read_thread",
            &params.target_user,
            &params.thread_id,
        );
        let maildir_root =
            std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        let store = mailrs_message_store::MaildirStore;
        let mut items = Vec::with_capacity(resp.items.len());
        for w in resp.items {
            let msg = crate::handlers::conversations::enrich_with_body_public(
                &store,
                &maildir_root,
                &params.target_user,
                w,
            )
            .await;
            items.push(serde_json::json!({
                "uid": msg.uid,
                "sender": msg.sender,
                "recipients": msg.recipients,
                "subject": msg.subject,
                "internal_date": msg.internal_date,
                "text_body": msg.text_body,
                "attachments": msg.attachments,
                "message_id": msg.message_id,
            }));
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "target_user": params.target_user,
                "thread_id": params.thread_id,
                "messages": items,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Reply into an existing thread. Resolves In-Reply-To, the `Re:` subject, and the default recipient from the thread's last message, then enqueues via the same path as `send_email`."
    )]
    async fn reply_email(
        &self,
        Parameters(params): Parameters<ReplyEmailParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let from = params.from.unwrap_or_else(|| user.clone());
        let thread = self
            .state
            .core
            .list_thread_messages(&user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("list_thread_messages: {e}"), None))?;
        let Some(last) = thread.items.last() else {
            return Err(McpError::invalid_params("thread has no messages", None));
        };
        let subject = if last.subject.to_lowercase().starts_with("re:") {
            last.subject.clone()
        } else {
            format!("Re: {}", last.subject)
        };
        let to = match params.to {
            Some(list) if !list.is_empty() => list,
            _ => vec![last.sender.clone()],
        };
        let cc = params.cc.unwrap_or_default();
        if to.len() + cc.len() > 50 {
            return Err(McpError::invalid_params(
                "too many recipients (max 50)",
                None,
            ));
        }
        let in_reply_to = last.message_id.clone();
        let message_id = crate::handlers::prefs::send_email_mcp(
            &self.state,
            &user,
            &from,
            &to,
            &cc,
            &subject,
            &params.body,
            Some(in_reply_to.as_str()),
            None,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("send: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "ok": true,
                "message_id": message_id,
                "thread_id": params.thread_id,
                "in_reply_to": in_reply_to,
                "subject": subject,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Schedule an email for future delivery. Same as `send_email` with a mandatory future `scheduled_at` (Unix epoch seconds). Manage the result with `list_own_scheduled` / `reschedule_scheduled` / `cancel_scheduled`."
    )]
    async fn send_scheduled_email(
        &self,
        Parameters(params): Parameters<SendScheduledEmailParams>,
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
        if params.scheduled_at <= now_secs() {
            return Err(McpError::invalid_params(
                "scheduled_at must be a future Unix epoch (seconds)",
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
            None,
            Some(params.scheduled_at),
        )
        .await
        .map_err(|e| McpError::internal_error(format!("schedule: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "ok": true,
                "message_id": message_id,
                "scheduled_at": params.scheduled_at,
            })
            .to_string(),
        )]))
    }
}
