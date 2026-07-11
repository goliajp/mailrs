//! v2.0.0 MCP tool batch 9 — snooze / unsnooze / pin / unpin / dismiss
//! thread mutation tools. Wraps the fastcore RPC surface at
//! `state.core.snooze_thread` etc.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SnoozeParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
    /// Snooze-until as Unix seconds. `0` means "indefinitely until
    /// cleared". Set to a future epoch to auto-wake at that time.
    pub until: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadIdOnly {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

#[tool_router(router = tool_router_v2_batch9, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Snooze a thread. `until` is Unix seconds — the thread re-surfaces at that epoch. `until = 0` means snooze indefinitely."
    )]
    async fn snooze_thread(
        &self,
        Parameters(params): Parameters<SnoozeParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let req = mailrs_core_api::method::thread::SnoozeRequest {
            snoozed_until: params.until,
        };
        self.state
            .core
            .snooze_thread(user, &params.thread_id, &req)
            .await
            .map_err(|e| McpError::internal_error(format!("snooze_thread: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "ok": true,
                "thread_id": params.thread_id,
                "snoozed_until": params.until,
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Clear a thread's snooze immediately.")]
    async fn unsnooze_thread(
        &self,
        Parameters(params): Parameters<ThreadIdOnly>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        self.state
            .core
            .unsnooze_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unsnooze_thread: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "ok": true, "thread_id": params.thread_id }).to_string(),
        )]))
    }

    #[tool(description = "Pin a thread to the top of every folder view.")]
    async fn pin_thread(
        &self,
        Parameters(params): Parameters<ThreadIdOnly>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let _ = self
            .state
            .core
            .pin_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("pin_thread: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "ok": true, "thread_id": params.thread_id }).to_string(),
        )]))
    }

    #[tool(description = "Unpin a thread.")]
    async fn unpin_thread(
        &self,
        Parameters(params): Parameters<ThreadIdOnly>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let _ = self
            .state
            .core
            .unpin_thread(user, &params.thread_id)
            .await
            .map_err(|e| McpError::internal_error(format!("unpin_thread: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "ok": true, "thread_id": params.thread_id }).to_string(),
        )]))
    }
}
