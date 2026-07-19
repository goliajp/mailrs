//! Parity batch 2 — thread state mutations (pin / snooze / bulk read).
//! Mirrors fastcore's v2 batch 9 plus its `mark_all_read` tool.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct MarkAllReadParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct PinThreadParams {
    /// Thread ID as returned by list_conversations.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SnoozeThreadParams {
    /// Thread ID as returned by list_conversations.
    pub thread_id: String,
    /// Snooze-until as Unix seconds. Must be a future epoch.
    pub until: i64,
}

#[tool_router(router = tool_router_parity2, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(description = "Mark every conversation as read in one call.")]
    async fn mark_all_read(
        &self,
        Parameters(_params): Parameters<MarkAllReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        let user = &self.auth_user.address;
        // no bulk primitive in the pg store — walk the unread
        // conversations and reuse mark_thread_read so modseq bumps and
        // IMAP notifications stay identical to the per-thread path
        let threads = mb_store
            .list_conversations(
                user,
                1000,
                None,
                None,
                None,
                false,
                None,
                Some(true),
                None,
                None,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("list_conversations: {e}"), None))?;
        let mut marked = 0u32;
        for t in &threads {
            marked += mb_store
                .mark_thread_read(user, &t.thread_id, None)
                .await
                .map_err(|e| McpError::internal_error(format!("mark_thread_read: {e}"), None))?;
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "status": "marked_all_read",
                "threads": threads.len(),
                "messages": marked,
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Pin a thread to the top of every folder view.")]
    async fn pin_thread(
        &self,
        Parameters(params): Parameters<PinThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .pin_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("pin_thread: {e}"), None))?;
        self.ok_result("pinned", &params.thread_id)
    }

    #[tool(description = "Unpin a thread.")]
    async fn unpin_thread(
        &self,
        Parameters(params): Parameters<PinThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .unpin_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unpin_thread: {e}"), None))?;
        self.ok_result("unpinned", &params.thread_id)
    }

    #[tool(
        description = "Snooze a thread. `until` is Unix seconds — the thread re-surfaces at that epoch."
    )]
    async fn snooze_thread(
        &self,
        Parameters(params): Parameters<SnoozeThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let until = chrono::DateTime::from_timestamp(params.until, 0)
            .ok_or_else(|| McpError::invalid_params("until is not a valid unix epoch", None))?;
        let mb_store = self.mb_store()?;
        mb_store
            .snooze_thread(&self.auth_user.address, &params.thread_id, until)
            .await
            .map_err(|e| McpError::internal_error(format!("snooze_thread: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "status": "snoozed",
                "thread_id": params.thread_id,
                "snoozed_until": params.until,
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Clear a thread's snooze immediately.")]
    async fn unsnooze_thread(
        &self,
        Parameters(params): Parameters<PinThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let mb_store = self.mb_store()?;
        mb_store
            .unsnooze_thread(&self.auth_user.address, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unsnooze_thread: {e}"), None))?;
        self.ok_result("unsnoozed", &params.thread_id)
    }
}
