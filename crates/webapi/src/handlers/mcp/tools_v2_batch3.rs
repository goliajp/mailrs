//! v2.0.0 MCP tool batch 3 — user-facing outbound queue control:
//! cancel and reschedule the current caller's scheduled sends.
//! Mirrors the REST endpoints added in Stage D (G13.3).

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScheduledIdParams {
    /// Outbound queue id as returned by `list_scheduled_outbound` /
    /// the compose-send endpoint. The id is unique across the whole
    /// scheduled zset; caller must own the envelope (sender field on
    /// the stored blob) — a mismatch returns `not_found`.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RescheduleParams {
    /// Outbound queue id.
    pub id: String,
    /// New send-time as Unix seconds. Must be strictly in the future.
    pub scheduled_at: i64,
}

#[tool_router(router = tool_router_v2_batch3, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Cancel one of the caller's own future-dated outbound sends. Removes the id from the scheduled zset and drops its envelope. Returns { ok: true } on success, or { ok: false, reason } if the id doesn't exist or the caller isn't the sender."
    )]
    async fn cancel_scheduled(
        &self,
        Parameters(params): Parameters<ScheduledIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let removed = match params.id.parse::<i64>() {
            Ok(id) => with_kevy(move |c| {
                mailrs_core_sidestate::families::outbound::cancel_scheduled(c, id, &user)
            })
            .unwrap_or(false),
            Err(_) => false,
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(
            if removed {
                serde_json::json!({ "ok": true }).to_string()
            } else {
                serde_json::json!({
                    "ok": false,
                    "reason": "id not found or caller is not the sender",
                })
                .to_string()
            },
        )]))
    }

    #[tool(
        description = "Reschedule one of the caller's own future-dated outbound sends. `scheduled_at` must be a future Unix-second epoch. Returns { ok: true } on success."
    )]
    async fn reschedule_scheduled(
        &self,
        Parameters(params): Parameters<RescheduleParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if params.scheduled_at <= now {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::json!({ "ok": false, "reason": "scheduled_at must be in the future" })
                    .to_string(),
            )]));
        }
        let at = params.scheduled_at;
        let ok = match params.id.parse::<i64>() {
            Ok(id) => with_kevy(move |c| {
                mailrs_core_sidestate::families::outbound::reschedule(c, id, &user, at)
            })
            .unwrap_or(false),
            Err(_) => false,
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(if ok {
            serde_json::json!({ "ok": true, "scheduled_at": params.scheduled_at }).to_string()
        } else {
            serde_json::json!({
                "ok": false,
                "reason": "id not found or caller is not the sender",
            })
            .to_string()
        })]))
    }
}
