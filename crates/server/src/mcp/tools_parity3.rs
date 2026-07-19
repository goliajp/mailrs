//! Parity batch 3 — the caller's own drafts. Same `drafts` table and
//! same statements as the REST handlers in `web::mail::drafts`.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListDraftsParams {
    /// Max drafts to return (newest first). Omit for all.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveDraftParams {
    /// Existing draft id to update in place. Omit to insert a new one.
    #[serde(default)]
    pub id: Option<i64>,
    /// To recipients, comma separated.
    #[serde(default)]
    pub to: String,
    /// CC recipients, comma separated.
    #[serde(default)]
    pub cc: String,
    /// BCC recipients, comma separated.
    #[serde(default)]
    pub bcc: String,
    /// Draft subject.
    #[serde(default)]
    pub subject: String,
    /// Draft body.
    #[serde(default)]
    pub body: String,
    /// Thread this draft replies to, when it is a reply.
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DraftIdParams {
    /// Draft id returned by list_drafts / save_draft.
    pub id: i64,
}

/// `drafts` projection: id, to, cc, bcc, subject, body,
/// reply_to_thread_id, created_at, updated_at.
type DraftRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    i64,
);

#[tool_router(router = tool_router_parity3, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(description = "List the caller's own saved drafts.")]
    async fn list_drafts(
        &self,
        Parameters(params): Parameters<ListDraftsParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pg_pool()?;
        let rows: Vec<DraftRow> = sqlx::query_as(
                "SELECT id, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id, \
                 EXTRACT(EPOCH FROM created_at)::bigint, EXTRACT(EPOCH FROM updated_at)::bigint \
                 FROM drafts WHERE user_address = $1 ORDER BY updated_at DESC",
            )
            .bind(&self.auth_user.address)
            .fetch_all(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("list_drafts: {e}"), None))?;
        let mut items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.0, "to": r.1, "cc": r.2, "bcc": r.3,
                    "subject": r.4, "body": r.5, "reply_to_thread_id": r.6,
                    "created_at": r.7, "updated_at": r.8,
                })
            })
            .collect();
        if let Some(limit) = params.limit {
            items.truncate(limit as usize);
        }
        self.json_result(&items)
    }

    #[tool(
        description = "Save a draft for the caller. Pass `id` to update an existing draft in place; omit it to create a new one. Returns the draft id."
    )]
    async fn save_draft(
        &self,
        Parameters(params): Parameters<SaveDraftParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.body.len() > crate::web::MAX_EMAIL_BODY_LEN {
            return Err(McpError::invalid_params("draft body too large", None));
        }
        if params.subject.len() > crate::web::MAX_ADMIN_FIELD_LEN {
            return Err(McpError::invalid_params("subject too long", None));
        }
        let pool = self.pg_pool()?;
        let user = &self.auth_user.address;

        // upsert: a known id updates in place (scoped to the caller); a
        // missing or stale id falls through to an insert
        let mut updated: Option<i64> = None;
        if let Some(id) = params.id {
            updated = sqlx::query_scalar::<_, i64>(
                "UPDATE drafts SET to_addresses = $3, cc_addresses = $4, bcc_addresses = $5, \
                 subject = $6, body = $7, reply_to_thread_id = $8, updated_at = now() \
                 WHERE id = $1 AND user_address = $2 RETURNING id",
            )
            .bind(id)
            .bind(user)
            .bind(&params.to)
            .bind(&params.cc)
            .bind(&params.bcc)
            .bind(&params.subject)
            .bind(&params.body)
            .bind(&params.reply_to_thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("save_draft: {e}"), None))?;
        }

        let id = match updated {
            Some(id) => id,
            None => sqlx::query_scalar::<_, i64>(
                "INSERT INTO drafts (user_address, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
            )
            .bind(user)
            .bind(&params.to)
            .bind(&params.cc)
            .bind(&params.bcc)
            .bind(&params.subject)
            .bind(&params.body)
            .bind(&params.reply_to_thread_id)
            .fetch_one(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("save_draft: {e}"), None))?,
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "id": id }).to_string(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own drafts by id.")]
    async fn delete_draft(
        &self,
        Parameters(params): Parameters<DraftIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pg_pool()?;
        let deleted = sqlx::query("DELETE FROM drafts WHERE id = $1 AND user_address = $2")
            .bind(params.id)
            .bind(&self.auth_user.address)
            .execute(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("delete_draft: {e}"), None))?
            .rows_affected();
        if deleted == 0 {
            return Err(McpError::invalid_params("draft not found", None));
        }
        self.ok_result("draft_deleted", &params.id.to_string())
    }
}
