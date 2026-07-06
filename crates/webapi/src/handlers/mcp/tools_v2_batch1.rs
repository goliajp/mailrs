//! v2.0.0 MCP tool batch 1 — admin-gated read tools that were missing
//! from the v1 fastcore surface. Every tool wraps an existing REST
//! handler's data source (network kevy admin:* hashes) so agents can
//! read the same rows the admin UI shows.
//!
//! Kept in a single named-router file (`tool_router_v2_batch1`) so
//! `mcp/mod.rs` combines it with `Self::tool_router_v1() + ...` in
//! one place. Adding more admin-read tools = extend this impl block.
//! Adding admin-write / user-CRUD tools → new sibling file so we stay
//! under the 500-line-per-file hard limit.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

fn admin_read_hash(key: &[u8]) -> Result<Vec<serde_json::Value>, McpError> {
    let flat =
        with_kevy(|c| c.hgetall(key)).map_err(|_| McpError::internal_error("kevy read", None))?;
    Ok(flat
        .chunks(2)
        .filter_map(|p| p.get(1))
        .filter_map(|v| serde_json::from_slice(v).ok())
        .collect())
}

#[tool_router(router = tool_router_v2_batch1, vis = "pub")]
impl MailrsMcpService {
    #[tool(description = "List all permission groups (admin-gated).")]
    async fn list_groups(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let items = admin_read_hash(b"admin:groups")?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(description = "List every registered OAuth-style admin app (admin-gated).")]
    async fn list_apps(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let items = admin_read_hash(b"admin:apps")?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(description = "List every email distribution group (admin-gated).")]
    async fn list_email_groups(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let items = admin_read_hash(b"admin:email-groups")?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(description = "List the local greylist allow/block rules (admin-gated).")]
    async fn list_greylist_local(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let items = admin_read_hash(b"admin:greylist:local-lists")?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "List every alias (admin-gated). Complements the v1 add/remove_alias tools with a read-side."
    )]
    async fn list_aliases_admin(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let items = admin_read_hash(b"admin:aliases")?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }
}
