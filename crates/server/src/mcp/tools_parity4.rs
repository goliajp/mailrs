//! Parity batch 4 — the caller's own compose templates. Same
//! `email_templates` table and statements as `web::templates`.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use super::MailMcpService;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListTemplatesParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveTemplateParams {
    /// Template name — unique per user; re-saving the same name updates it.
    pub name: String,
    /// Default subject line.
    #[serde(default)]
    pub subject: String,
    /// HTML body.
    #[serde(default)]
    pub html_body: String,
    /// Plain-text body.
    #[serde(default)]
    pub text_body: String,
    /// Grouping category (defaults to "general").
    #[serde(default)]
    pub category: Option<String>,
    /// Make this the caller's default template (clears any previous default).
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct TemplateIdParams {
    /// Template id returned by list_templates / save_template.
    pub id: i64,
}

/// `email_templates` projection: id, name, subject, html_body, text_body,
/// category, is_default, created_at, updated_at.
type TemplateRow = (i64, String, String, String, String, String, bool, i64, i64);

#[tool_router(router = tool_router_parity4, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(description = "List the caller's own compose templates.")]
    async fn list_templates(
        &self,
        Parameters(_params): Parameters<ListTemplatesParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pg_pool()?;
        let rows: Vec<TemplateRow> = sqlx::query_as(
            "SELECT id, name, subject, html_body, text_body, category, is_default, \
                 EXTRACT(EPOCH FROM created_at)::bigint, EXTRACT(EPOCH FROM updated_at)::bigint \
                 FROM email_templates WHERE user_address = $1 \
                 ORDER BY is_default DESC, updated_at DESC",
        )
        .bind(&self.auth_user.address)
        .fetch_all(pool)
        .await
        .map_err(|e| McpError::internal_error(format!("list_templates: {e}"), None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.0, "name": r.1, "subject": r.2,
                    "html_body": r.3, "text_body": r.4, "category": r.5,
                    "is_default": r.6, "created_at": r.7, "updated_at": r.8,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Save or update the caller's own compose template. Keyed by name — re-saving the same name overwrites. Returns the template id."
    )]
    async fn save_template(
        &self,
        Parameters(params): Parameters<SaveTemplateParams>,
    ) -> Result<CallToolResult, McpError> {
        let name = params.name.trim();
        if name.is_empty() {
            return Err(McpError::invalid_params("name is required", None));
        }
        let pool = self.pg_pool()?;
        let user = &self.auth_user.address;
        let category = params.category.as_deref().unwrap_or("general");

        // a partial unique index enforces at most one default per user,
        // so clear the previous default before claiming it
        if params.is_default {
            sqlx::query(
                "UPDATE email_templates SET is_default = false WHERE user_address = $1 AND is_default = true",
            )
            .bind(user)
            .execute(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("save_template: {e}"), None))?;
        }

        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO email_templates (user_address, name, subject, html_body, text_body, category, is_default, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
             ON CONFLICT (user_address, name) DO UPDATE SET \
               subject = EXCLUDED.subject, html_body = EXCLUDED.html_body, \
               text_body = EXCLUDED.text_body, category = EXCLUDED.category, \
               is_default = EXCLUDED.is_default, updated_at = now() \
             RETURNING id",
        )
        .bind(user)
        .bind(name)
        .bind(&params.subject)
        .bind(&params.html_body)
        .bind(&params.text_body)
        .bind(category)
        .bind(params.is_default)
        .fetch_one(pool)
        .await
        .map_err(|e| McpError::internal_error(format!("save_template: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "id": id, "name": name }).to_string(),
        )]))
    }

    #[tool(description = "Delete one of the caller's own compose templates by id.")]
    async fn delete_template(
        &self,
        Parameters(params): Parameters<TemplateIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pg_pool()?;
        let deleted =
            sqlx::query("DELETE FROM email_templates WHERE id = $1 AND user_address = $2")
                .bind(params.id)
                .bind(&self.auth_user.address)
                .execute(pool)
                .await
                .map_err(|e| McpError::internal_error(format!("delete_template: {e}"), None))?
                .rows_affected();
        if deleted == 0 {
            return Err(McpError::invalid_params("template not found", None));
        }
        self.ok_result("template_deleted", &params.id.to_string())
    }
}
