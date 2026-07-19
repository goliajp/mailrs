//! Parity batch 6 — outbound queue stats + failed-delivery holding
//! area. The list form of the queue lives in `mod.rs` as
//! `list_admin_queue`; `get_queue` here is the cheap counter.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;
use mailrs_outbound_queue::queue::{self, QueueStatus};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetQueueStatsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListFailedOutboundParams {}

/// how many recent queue rows the failed listing scans
const QUEUE_SCAN_LIMIT: i32 = 500;

#[tool_router(router = tool_router_parity6, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(description = "Outbound queue stats (pending count).")]
    async fn get_queue(
        &self,
        Parameters(_params): Parameters<GetQueueStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.outbound_pool()?;
        let pending = queue::count_pending(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("count_pending: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "pending": pending }).to_string(),
        )]))
    }

    #[tool(
        description = "List outbound deliveries that hit a terminal error and are held for operator inspection (failed + bounced). Requires admin.queue permission."
    )]
    async fn list_failed_outbound(
        &self,
        Parameters(_params): Parameters<ListFailedOutboundParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self.outbound_pool()?;
        let entries = queue::list_recent(pool, QUEUE_SCAN_LIMIT)
            .await
            .map_err(|e| McpError::internal_error(format!("list_recent: {e}"), None))?;
        let items: Vec<serde_json::Value> = entries
            .iter()
            .filter(|m| matches!(m.status, QueueStatus::Failed | QueueStatus::Bounced))
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "sender": m.sender,
                    "recipient": m.recipient,
                    "domain": m.domain,
                    "status": m.status.as_str(),
                    "attempts": m.attempts,
                    "last_error": m.last_error,
                    "updated_at": m.updated_at,
                })
            })
            .collect();
        self.json_result(&items)
    }
}
