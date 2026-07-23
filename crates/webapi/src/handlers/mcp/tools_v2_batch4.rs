//! v2.0.0 MCP tool batch 4 — self-introspection tools that make an
//! agent's own permissions + own scheduled-send inventory visible
//! without needing admin gates.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[tool_router(router = tool_router_v2_batch4, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Return the authenticated caller's own effective permissions (is_super, admin.*, send_as, etc.). No admin gate. Useful for an agent to check what actions it's allowed to take before attempting them."
    )]
    async fn get_my_permissions(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?;
        let perms = self
            .state
            .core
            .effective_permissions(user)
            .await
            .map_err(|e| McpError::internal_error(format!("effective_permissions: {e}"), None))?;
        // effective_permissions returns the wire struct; serialize it
        // and return alongside the resolved user so the agent has a
        // whole answer without a follow-up whoami call.
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "user": user,
                "permissions": perms,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "List the caller's own future-dated scheduled sends. Filters mailrs:outbound:scheduled-idx to entries where the envelope's sender field matches the authenticated user. No admin required."
    )]
    async fn list_own_scheduled(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let raw = with_kevy(|c| c.zrange(b"mailrs:outbound:scheduled-idx", 0, -1))
            .map_err(|_| McpError::internal_error("scheduled zset read", None))?;
        let user_c = user.clone();
        let items: Vec<serde_json::Value> = with_kevy(move |c| {
            let mut out = Vec::new();
            for m in raw {
                let Ok(id) = String::from_utf8(m.clone()) else {
                    continue;
                };
                let hkey = format!("mailrs:outbound:job:{id}");
                let Some(blob) = c.hget(hkey.as_bytes(), b"blob")? else {
                    continue;
                };
                let Ok(env) = serde_json::from_slice::<serde_json::Value>(&blob) else {
                    continue;
                };
                if env.get("sender").and_then(|v| v.as_str()) != Some(user_c.as_str()) {
                    continue;
                }
                let score = c
                    .zscore(b"mailrs:outbound:scheduled-idx", &m)?
                    .unwrap_or(0.0) as i64;
                out.push(serde_json::json!({
                    "id": id,
                    "scheduled_at": score,
                    "sender": env.get("sender").and_then(|v| v.as_str()),
                    "recipient": env.get("recipient").and_then(|v| v.as_str()),
                    "subject": env.get("subject").and_then(|v| v.as_str()),
                }));
            }
            Ok(out)
        })
        .map_err(|_| McpError::internal_error("scheduled zset filter", None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }
}
