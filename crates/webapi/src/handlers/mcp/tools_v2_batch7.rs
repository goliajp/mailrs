//! v2.0.0 MCP tool batch 7 — server introspection + failed-message
//! retry. Reasonable place to add general-purpose util tools that
//! don't fit any other category.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RetryQueueParams {
    /// Outbound queue id — as returned by `list_failed_outbound`.
    pub id: String,
}

#[tool_router(router = tool_router_v2_batch7, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Return the mailrs server version (as compiled — matches `curl /api/health`'s server-header). Useful for an agent to check feature availability before falling back."
    )]
    async fn get_server_info(&self) -> Result<CallToolResult, McpError> {
        let _ = self.require_user()?;
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
        description = "Retry a failed outbound message by id. Moves the id from `mailrs:outbound:failed` back onto `mailrs:outbound:pending-idx` (state=pending on the v2 job hash). Admin-gated — returns { ok: false, reason } if the id isn't in the failed set."
    )]
    async fn retry_queue_message(
        &self,
        Parameters(params): Parameters<RetryQueueParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let id = params.id;
        let id_c = id.clone();
        let ok = with_kevy(move |c| {
            let members = c.smembers(b"mailrs:outbound:failed")?;
            let present = members.iter().any(|m| m == id_c.as_bytes());
            if !present {
                return Ok(false);
            }
            c.srem(b"mailrs:outbound:failed", &[id_c.as_bytes()])?;
            // v2 requeue — legacy pending list is dead; sender only
            // reads pending-idx.
            let id_i64: i64 = std::str::from_utf8(id_c.as_bytes())
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| std::io::Error::other("id not i64"))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            mailrs_core_sidestate::families::outbound::requeue_pending(c, id_i64, now).map(|_| true)
        })
        .unwrap_or(false);
        Ok(CallToolResult::success(vec![Content::text(if ok {
            serde_json::json!({ "ok": true, "id": id, "requeued": "pending" }).to_string()
        } else {
            serde_json::json!({
                "ok": false,
                "reason": "id not present in mailrs:outbound:failed",
            })
            .to_string()
        })]))
    }
}
