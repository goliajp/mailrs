//! v2.0.0 MCP tool batch 6 — outbound queue admin introspection.
//! Mirrors the REST `list_admin_queue` handler at
//! `handlers::complete::list_admin_queue`. Admin-gated read-only.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[tool_router(router = tool_router_v2_batch6, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List the current outbound queue (last 100 IDs from pending + inflight) with each envelope's sender / recipient / subject. Admin-gated read."
    )]
    async fn list_admin_queue(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let (pending_ids, inflight_ids): (Vec<Vec<u8>>, Vec<Vec<u8>>) = with_kevy(|c| {
            let pending = c
                .lrange(b"mailrs:outbound:pending", 0, 99)
                .unwrap_or_default();
            let inflight = c
                .lrange(b"mailrs:outbound:inflight", 0, 99)
                .unwrap_or_default();
            Ok((pending, inflight))
        })
        .map_err(|_| McpError::internal_error("queue list read", None))?;
        let mut items = Vec::new();
        for (label, ids) in [("pending", &pending_ids), ("inflight", &inflight_ids)] {
            for b in ids {
                let id_str = String::from_utf8_lossy(b).to_string();
                let key = format!("mailrs:outbound:{id_str}");
                let key_c = key.clone();
                let blob = match with_kevy(move |c| c.hget(key_c.as_bytes(), b"blob")) {
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
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "List IDs on the outbound `failed` set (deliveries that hit a terminal error and are held for operator inspection). Admin-gated."
    )]
    async fn list_failed_outbound(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let raw = with_kevy(|c| c.smembers(b"mailrs:outbound:failed"))
            .map_err(|_| McpError::internal_error("failed set read", None))?;
        let ids: Vec<String> = raw
            .into_iter()
            .filter_map(|v| String::from_utf8(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": ids }).to_string(),
        )]))
    }
}
