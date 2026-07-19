//! Parity batch 1 — self / server introspection plus the cheap
//! dashboard reads. Mirrors fastcore's v2 batches 4, 7, 8 and 10.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;
use mailrs_mailbox::types::FLAG_SEEN;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetMyPermissionsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetServerInfoParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetInboxMetricsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetThreadSummaryParams {
    /// Thread ID as returned by list_conversations.
    pub thread_id: String,
}

#[tool_router(router = tool_router_parity1, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "Return the authenticated caller's own effective permissions (is_super, admin.*, send_as, etc.). No admin gate. Useful for an agent to check what actions it's allowed to take before attempting them."
    )]
    async fn get_my_permissions(
        &self,
        Parameters(_params): Parameters<GetMyPermissionsParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "user": self.auth_user.address,
                "permissions": self.auth_user.permissions.as_ref(),
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Return the mailrs server version (as compiled — matches `curl /api/health`'s server header). Useful for an agent to check feature availability before falling back."
    )]
    async fn get_server_info(
        &self,
        Parameters(_params): Parameters<GetServerInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "product": "mailrs",
                "mcp_protocol": "2025-03-26",
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Return the caller's unseen-message count. Cheap dashboard metric — no bodies loaded. Useful for an agent deciding whether to sift or leave the inbox alone."
    )]
    async fn get_inbox_metrics(
        &self,
        Parameters(_params): Parameters<GetInboxMetricsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let unseen = mb_store
            .count_unseen(&self.auth_user.address)
            .await
            .map_err(|e| McpError::internal_error(format!("count_unseen: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "user": self.auth_user.address,
                "unseen_count": unseen,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Fetch a lightweight summary of a thread — subject, participants, message count, unread count, last_date — without loading full message bodies. Faster than read_thread for triage / list previews."
    )]
    async fn get_thread_summary(
        &self,
        Parameters(params): Parameters<GetThreadSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let messages = mb_store
            .list_thread_messages(&self.auth_user.address, &params.thread_id, None)
            .await
            .map_err(|e| McpError::internal_error(format!("list_thread_messages: {e}"), None))?;
        let participants: Vec<String> = messages
            .iter()
            .map(|m| m.sender.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let subject = messages
            .last()
            .map(|m| m.subject.clone())
            .unwrap_or_default();
        let last_date = messages.iter().map(|m| m.internal_date).max().unwrap_or(0);
        let unread_count = messages
            .iter()
            .filter(|m| (m.flags & FLAG_SEEN) == 0)
            .count();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "thread_id": params.thread_id,
                "subject": subject,
                "participants": participants,
                "message_count": messages.len(),
                "unread_count": unread_count,
                "last_date": last_date,
            })
            .to_string(),
        )]))
    }
}
