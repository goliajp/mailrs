//! v2.0.0 MCP tool batch 2 — misc admin read tools (queue introspection,
//! group membership, maildir reconcile). Continues the split laid out
//! in `tools_v2_batch1.rs`.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmailGroupIdParams {
    /// Email group id (e.g. `1`, `2` — returned by `list_email_groups`).
    pub id: String,
}

#[tool_router(router = tool_router_v2_batch2, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Scan the maildir tree and count message files per user. Read-only — reports discrepancies vs. the fastcore index, does not repair. Admin-gated."
    )]
    async fn reconcile_maildir(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        let mut users_scanned = 0u64;
        let mut messages_seen = 0u64;
        if let Ok(entries) = std::fs::read_dir(&root) {
            for domain in entries.flatten() {
                if !domain.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                if let Ok(user_dirs) = std::fs::read_dir(domain.path()) {
                    for u in user_dirs.flatten() {
                        if !u.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            continue;
                        }
                        users_scanned += 1;
                        for sub in ["new", "cur"] {
                            let p = u.path().join(sub);
                            if let Ok(msgs) = std::fs::read_dir(&p) {
                                messages_seen += msgs.count() as u64;
                            }
                        }
                    }
                }
            }
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "users_scanned": users_scanned,
                "messages_seen": messages_seen,
                "maildir_root": root,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "List every outbound message currently in the scheduled zset (future-dated sends). Returns id + scheduled_at epoch. Admin-gated."
    )]
    async fn list_scheduled_outbound(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let raw = with_kevy(|c| c.zrange(b"mailrs:outbound:scheduled", 0, -1))
            .map_err(|_| McpError::internal_error("scheduled zset read", None))?;
        // zrange returns members without scores in kevy-client 1.13; fetch
        // score per member via zscore in the same connection.
        let items: Vec<serde_json::Value> = with_kevy(move |c| {
            let mut out = Vec::with_capacity(raw.len());
            for m in raw {
                let Ok(id) = String::from_utf8(m.clone()) else {
                    continue;
                };
                let score = c.zscore(b"mailrs:outbound:scheduled", &m)?.unwrap_or(0.0);
                out.push(serde_json::json!({ "id": id, "scheduled_at": score as i64 }));
            }
            Ok(out)
        })
        .map_err(|_| McpError::internal_error("scheduled zscore read", None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": items }).to_string(),
        )]))
    }

    #[tool(
        description = "List the members of a distribution email group by id. Admin-gated. Companion to list_email_groups."
    )]
    async fn get_email_group_members(
        &self,
        Parameters(params): Parameters<EmailGroupIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let key = format!("admin:email-group:{}:members", params.id);
        let members = with_kevy(move |c| c.smembers(key.as_bytes()))
            .map_err(|_| McpError::internal_error("kevy read", None))?;
        let items: Vec<String> = members
            .into_iter()
            .filter_map(|v| String::from_utf8(v).ok())
            .collect();
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "group_id": params.id, "items": items }).to_string(),
        )]))
    }
}
