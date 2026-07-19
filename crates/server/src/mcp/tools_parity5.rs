//! Parity batch 5 — scheduled outbound control. In the monolith lane a
//! "scheduled" send is an `outbound_queue` row still in `pending` whose
//! `next_retry` sits in the future (see `queue::enqueue_scheduled`), so
//! all four tools filter on that shape.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;
use mailrs_outbound_queue::queue::{self, QueueStatus, QueuedMessage};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListOwnScheduledParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListScheduledOutboundParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ScheduledIdParams {
    /// Outbound queue id as returned by list_own_scheduled.
    pub id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RescheduleScheduledParams {
    /// Outbound queue id.
    pub id: i64,
    /// New send time as Unix seconds. Must be strictly in the future.
    pub scheduled_at: i64,
}

/// how many recent queue rows the scheduled/failed listings scan
const QUEUE_SCAN_LIMIT: i32 = 500;

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn is_scheduled(m: &QueuedMessage, now: i64) -> bool {
    m.status == QueueStatus::Pending && m.next_retry > now
}

fn scheduled_json(m: &QueuedMessage) -> serde_json::Value {
    serde_json::json!({
        "id": m.id,
        "scheduled_at": m.next_retry,
        "sender": m.sender,
        "recipient": m.recipient,
        "message_id": m.message_id,
        "created_at": m.created_at,
    })
}

#[tool_router(router = tool_router_parity5, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "List the caller's own future-dated scheduled sends. No admin required — filtered to rows whose sender is the authenticated user."
    )]
    async fn list_own_scheduled(
        &self,
        Parameters(_params): Parameters<ListOwnScheduledParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.outbound_pool()?;
        let now = now_epoch();
        let entries = queue::list_recent(pool, QUEUE_SCAN_LIMIT)
            .await
            .map_err(|e| McpError::internal_error(format!("list_recent: {e}"), None))?;
        let items: Vec<serde_json::Value> = entries
            .iter()
            .filter(|m| is_scheduled(m, now) && m.sender == self.auth_user.address)
            .map(scheduled_json)
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "List every outbound message currently scheduled for a future send, across all senders. Returns id + scheduled_at epoch. Requires admin.queue permission."
    )]
    async fn list_scheduled_outbound(
        &self,
        Parameters(_params): Parameters<ListScheduledOutboundParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self.outbound_pool()?;
        let now = now_epoch();
        let entries = queue::list_recent(pool, QUEUE_SCAN_LIMIT)
            .await
            .map_err(|e| McpError::internal_error(format!("list_recent: {e}"), None))?;
        let items: Vec<serde_json::Value> = entries
            .iter()
            .filter(|m| is_scheduled(m, now))
            .map(scheduled_json)
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Cancel one of the caller's own future-dated outbound sends. Returns { ok: false, reason } if the id doesn't exist or the caller isn't the sender."
    )]
    async fn cancel_scheduled(
        &self,
        Parameters(params): Parameters<ScheduledIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.outbound_pool()?;
        let msg = queue::get_message(pool, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("get_message: {e}"), None))?;
        let owned = msg
            .as_ref()
            .is_some_and(|m| m.sender == self.auth_user.address);
        if !owned {
            return Ok(not_owned());
        }
        let cancelled = queue::cancel_pending(pool, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("cancel_pending: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            match cancelled {
                true => serde_json::json!({ "ok": true, "id": params.id }).to_string(),
                false => serde_json::json!({
                    "ok": false,
                    "reason": "message is no longer pending",
                })
                .to_string(),
            },
        )]))
    }

    #[tool(
        description = "Reschedule one of the caller's own future-dated outbound sends. `scheduled_at` must be a future Unix-second epoch."
    )]
    async fn reschedule_scheduled(
        &self,
        Parameters(params): Parameters<RescheduleScheduledParams>,
    ) -> Result<CallToolResult, McpError> {
        let now = now_epoch();
        if params.scheduled_at <= now {
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({
                    "ok": false,
                    "reason": "scheduled_at must be in the future",
                })
                .to_string(),
            )]));
        }
        let pool = self.outbound_pool()?;
        let msg = queue::get_message(pool, params.id)
            .await
            .map_err(|e| McpError::internal_error(format!("get_message: {e}"), None))?;
        let owned = msg
            .as_ref()
            .is_some_and(|m| m.sender == self.auth_user.address);
        if !owned {
            return Ok(not_owned());
        }
        let moved = queue::reschedule_pending(pool, params.id, params.scheduled_at, now)
            .await
            .map_err(|e| McpError::internal_error(format!("reschedule_pending: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(match moved {
            true => serde_json::json!({
                "ok": true,
                "id": params.id,
                "scheduled_at": params.scheduled_at,
            })
            .to_string(),
            false => serde_json::json!({
                "ok": false,
                "reason": "message is no longer pending",
            })
            .to_string(),
        })]))
    }
}

fn not_owned() -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        serde_json::json!({
            "ok": false,
            "reason": "id not found or caller is not the sender",
        })
        .to_string(),
    )])
}
