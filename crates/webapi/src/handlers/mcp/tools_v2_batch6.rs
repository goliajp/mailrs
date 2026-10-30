//! v2.0.0 MCP tool batch 6 — outbound queue administration: the queue,
//! the failed set, and the accounts whose outbound mail is paused (the
//! REST `admin_outbound_pause` routes). Super-administrator only.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use super::params::ResumeOutboundParams;
use crate::handlers::kevy_util::with_kevy;
use mailrs_core_sidestate::families::outbound::pause;
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, RETRY_IDX};
use rmcp::handler::server::wrapper::Parameters;

#[tool_router(router = tool_router_v2_batch6, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List the current outbound queue (up to 100 IDs each from pending, retrying and inflight) with each envelope's sender / recipient / subject. Super-administrator only."
    )]
    async fn list_admin_queue(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_super(&user).await?;
        let (pending_ids, retrying_ids, inflight_ids) = with_kevy(|c| {
            let pending = c.lrange(PENDING_IDX, 0, 99).unwrap_or_default();
            let retrying = c.zrange(RETRY_IDX, 0, 99).unwrap_or_default();
            let inflight = c
                .lrange(mailrs_core_sidestate::families::outbound::INFLIGHT, 0, 99)
                .unwrap_or_default();
            Ok((pending, retrying, inflight))
        })
        .map_err(|_| McpError::internal_error("queue list read", None))?;
        let mut items = Vec::new();
        for (label, ids) in [
            ("pending", &pending_ids),
            ("retrying", &retrying_ids),
            ("inflight", &inflight_ids),
        ] {
            for b in ids {
                let id_str = String::from_utf8_lossy(b).to_string();
                let key = mailrs_core_sidestate::families::outbound::job_key(&id_str);
                let key_c = key.clone();
                let blob = match with_kevy(move |c| {
                    c.hget(key_c.as_bytes(), b"blob")
                        .map_err(std::io::Error::from)
                }) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(b) = blob
                    && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b)
                {
                    let mut item = v;
                    if let Some(o) = item.as_object_mut() {
                        o.insert("status".into(), serde_json::Value::String(label.into()));
                        o.insert("id".into(), serde_json::Value::String(id_str.clone()));
                    }
                    items.push(item);
                }
            }
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "List IDs on the outbound `failed` set (deliveries that hit a terminal error and are held for operator inspection). Super-administrator only."
    )]
    async fn list_failed_outbound(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_super(&user).await?;
        let raw = with_kevy(|c| {
            c.smembers(mailrs_core_sidestate::families::outbound::FAILED_SET)
                .map_err(std::io::Error::from)
        })
        .map_err(|_| McpError::internal_error("failed set read", None))?;
        let ids: Vec<String> = raw
            .into_iter()
            .filter_map(|v| String::from_utf8(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "items": ids }).to_string(),
        )]))
    }

    #[tool(
        description = "List the accounts whose outbound mail is paused (sent far past the hourly limit, or kept failing): address, reason, since when, and how many messages are held. Super-administrator only."
    )]
    async fn list_outbound_paused(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_super(&user).await?;
        let items = with_kevy(pause::paused)
            .map_err(|_| McpError::internal_error("paused list read", None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "Lift the pause on an account's outbound mail. `held` is `send` (the held messages go out) or `discard` (they are deleted unsent); the answer counts each. Super-administrator only, with a recent confirmation."
    )]
    async fn resume_outbound_paused(
        &self,
        Parameters(params): Parameters<ResumeOutboundParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_confirmed()?;
        self.require_super(&user).await?;
        let send = match params.held.as_str() {
            "send" => true,
            "discard" => false,
            other => {
                return Err(McpError::invalid_params(
                    format!("held must be send or discard, not {other:?}"),
                    None,
                ));
            }
        };
        let addr = params.address.to_ascii_lowercase();
        let now = chrono::Utc::now().timestamp();
        let done = with_kevy(move |c| pause::resume(c, &addr, send, now))
            .map_err(|_| McpError::internal_error("resume", None))?
            .ok_or_else(|| McpError::invalid_params("that account is not paused", None))?;
        let detail = format!("sent={} discarded={}", done.sent, done.discarded);
        crate::handlers::audit::record(&user, "outbound.resume", &params.address, &detail);
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::json!(done).to_string(),
        )]))
    }
}
