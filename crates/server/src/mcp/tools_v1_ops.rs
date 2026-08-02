//! v1 MCP tools over operations: greylist, webhooks, the outbound queue,
//! maildir reconcile, audit, encryption keys, system config.
//!
//! Split out of `mcp/mod.rs` on 2026-08-02, which held all sixty-one v1
//! tools in one 2,450-line file — the very thing
//! `.claude/rules/mcp-two-lane-parity.md` says not to keep doing. Tool
//! **names** are the wire contract and must match the fastcore lane;
//! `scripts/check-mcp-parity.sh` holds both to the same 82.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::MailMcpService;
use super::tools::*;

#[tool_router(router = tool_router_v1_ops, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "List local greylist whitelist/blacklist entries. Filter by kind (domain/email/cidr) or list (white/black). Requires admin.greylist."
    )]
    async fn list_greylist_local(
        &self,
        Parameters(params): Parameters<ListGreylistLocalParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.greylist")?;
        let pool = self.pool()?;
        let mut sql = "SELECT id, kind, list, value, note, \
             EXTRACT(EPOCH FROM created_at)::bigint, created_by \
             FROM greylist_local_lists WHERE 1=1"
            .to_string();
        let mut binds: Vec<String> = Vec::new();
        if let Some(ref k) = params.kind
            && matches!(k.as_str(), "domain" | "email" | "cidr")
        {
            binds.push(k.clone());
            sql.push_str(&format!(" AND kind = ${}", binds.len()));
        }
        if let Some(ref l) = params.list
            && matches!(l.as_str(), "white" | "black")
        {
            binds.push(l.clone());
            sql.push_str(&format!(" AND list = ${}", binds.len()));
        }
        sql.push_str(" ORDER BY id");
        let mut q = sqlx::query_as::<
            _,
            (
                i64,
                String,
                String,
                String,
                Option<String>,
                i64,
                Option<String>,
            ),
        >(&sql);
        for b in &binds {
            q = q.bind(b);
        }
        let rows = q
            .fetch_all(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, kind, list, value, note, created_at, created_by)| {
                serde_json::json!({
                    "id": id,
                    "kind": kind,
                    "list": list,
                    "value": value,
                    "note": note,
                    "created_at": created_at,
                    "created_by": created_by,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Add a local greylist entry. white entries bypass the greylist (skip triplet check); black entries are rejected with SMTP 550. A single key cannot exist on both lists at once — to move an entry, remove and re-add. kind is domain/email/cidr; list is white/black. Requires admin.greylist."
    )]
    async fn greylist_local_add(
        &self,
        Parameters(params): Parameters<GreylistLocalAddParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.greylist")?;
        let pool = self.pool()?;
        crate::greylist_local::validate_list(&params.list)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let normalized = crate::greylist_local::normalize(&params.kind, &params.value)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let actor = self.auth_user.address.clone();
        let res: Result<(i64,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO greylist_local_lists (kind, list, value, note, created_by)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(&params.kind)
        .bind(&params.list)
        .bind(&normalized)
        .bind(params.note.as_deref())
        .bind(actor.as_str())
        .fetch_one(pool)
        .await;
        match res {
            Ok((id,)) => {
                crate::greylist_local::reload(&self.web_state.greylist_local, pool).await;
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::json!({
                        "status": "greylist_local_added",
                        "id": id,
                        "kind": params.kind,
                        "list": params.list,
                        "value": normalized,
                    })
                    .to_string(),
                )]))
            }
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                Err(McpError::invalid_params(
                    format!(
                        "value '{normalized}' already exists in greylist_local_lists; \
                         remove the existing entry before re-adding to a different list"
                    ),
                    None,
                ))
            }
            Err(e) => Err(McpError::internal_error(format!("{e}"), None)),
        }
    }

    #[tool(
        description = "Remove a local greylist entry by id (from greylist_local_list). Requires admin.greylist."
    )]
    async fn greylist_local_remove(
        &self,
        Parameters(params): Parameters<GreylistLocalRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.greylist")?;
        let pool = self.pool()?;
        let r = sqlx::query("DELETE FROM greylist_local_lists WHERE id = $1")
            .bind(params.id)
            .execute(pool)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if r.rows_affected() == 0 {
            return Err(McpError::invalid_params(
                format!("id {} not found", params.id),
                None,
            ));
        }
        crate::greylist_local::reload(&self.web_state.greylist_local, pool).await;
        self.ok_result("greylist_local_removed", &params.id.to_string())
    }

    #[tool(description = "List your webhook subscriptions.")]
    async fn list_webhooks(
        &self,
        Parameters(_params): Parameters<ListWebhooksParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let subs = crate::webhook::store::list_subscriptions(pool, &self.auth_user.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = subs
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id, "url": s.url, "event_type": s.event_type,
                    "filter_sender": s.filter_sender, "filter_thread_id": s.filter_thread_id,
                    "active": s.active,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Create a webhook subscription for new email events. Returns the signing secret (save it, shown only once)."
    )]
    async fn create_webhook(
        &self,
        Parameters(params): Parameters<CreateWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let signing_secret = crate::webhook::store::generate_signing_secret();
        let id = crate::webhook::store::create_subscription(
            pool,
            &self.auth_user.address,
            &params.url,
            &params.event_type,
            params.filter_sender.as_deref(),
            params.filter_thread_id.as_deref(),
            &signing_secret,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "id": id, "url": params.url, "event_type": params.event_type,
                "signing_secret": signing_secret,
                "warning": "Save this signing secret now. It cannot be retrieved again.",
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Delete a webhook subscription by ID.")]
    async fn delete_webhook(
        &self,
        Parameters(params): Parameters<DeleteWebhookParams>,
    ) -> Result<CallToolResult, McpError> {
        let pool = self.pool()?;
        let removed =
            crate::webhook::store::delete_subscription(pool, params.id, &self.auth_user.address)
                .await
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("webhook_deleted", &params.id.to_string())
        } else {
            Err(McpError::invalid_params("webhook not found", None))
        }
    }

    #[tool(
        description = "List the current outbound queue (last 100 entries) with each envelope's sender / recipient / status. Requires admin.queue permission."
    )]
    async fn list_admin_queue(
        &self,
        Parameters(_params): Parameters<ListAdminQueueParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self
            .web_state
            .outbound_queue
            .as_ref()
            .ok_or_else(|| McpError::internal_error("outbound queue not configured", None))?;
        let entries = mailrs_outbound_queue::queue::list_recent(pool, 100)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = entries
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id, "sender": m.sender, "recipient": m.recipient,
                    "domain": m.domain, "status": m.status.as_str(),
                    "attempts": m.attempts, "last_error": m.last_error,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Reconcile maildir files into the message index (split-brain repair). dry_run reports the gap without writing. Requires internal.rpc permission."
    )]
    async fn reconcile_maildir(
        &self,
        Parameters(params): Parameters<ReconcileMaildirParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("internal.rpc")?;
        let store = self
            .web_state
            .mailbox_store
            .as_ref()
            .ok_or_else(|| McpError::internal_error("mailbox store not available", None))?;
        let report = store
            .reconcile_maildir(&self.web_state.maildir_root, params.dry_run)
            .await
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({"dry_run": params.dry_run, "report": report}).to_string(),
        )]))
    }

    #[tool(description = "Retry a failed outbound message. Requires admin.queue permission.")]
    async fn retry_queue_message(
        &self,
        Parameters(params): Parameters<RetryQueueMessageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.queue")?;
        let pool = self
            .web_state
            .outbound_queue
            .as_ref()
            .ok_or_else(|| McpError::internal_error("outbound queue not configured", None))?;
        let now = chrono::Utc::now().timestamp();
        let retried = mailrs_outbound_queue::queue::retry_message(pool, params.id, now)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if retried {
            self.ok_result("retrying", &params.id.to_string())
        } else {
            Err(McpError::invalid_params(
                "message not found or not retryable",
                None,
            ))
        }
    }

    #[tool(
        description = "Get all system configuration entries with current values, types, sources (database/env/default), and metadata. Requires admin.system_config permission."
    )]
    async fn get_system_config(
        &self,
        Parameters(_params): Parameters<GetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store =
            self.web_state.system_config.as_ref().ok_or_else(|| {
                McpError::internal_error("system config store not available", None)
            })?;

        let entries = store.get_all_entries();
        let items: Vec<serde_json::Value> = entries
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "value_type": e.value_type,
                    "group": e.group,
                    "description": e.description,
                    "source": e.source,
                    "updated_at": e.updated_at,
                    "updated_by": e.updated_by,
                })
            })
            .collect();

        self.json_result(&items)
    }

    #[tool(
        description = "Set a system configuration value. Validates key and value type. Requires admin.system_config permission. Use get_system_config to see available keys."
    )]
    async fn set_system_config(
        &self,
        Parameters(params): Parameters<SetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store =
            self.web_state.system_config.as_ref().ok_or_else(|| {
                McpError::internal_error("system config store not available", None)
            })?;

        store
            .set(&params.key, &params.value, &self.auth_user.address)
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;

        if let Some(ref ds) = self.web_state.domain_store {
            ds.log_audit(
                &self.auth_user.address,
                "system_config_updated",
                &params.key,
                &params.value,
            )
            .await;
        }

        self.ok_result("updated", &format!("{} = {}", params.key, params.value))
    }

    #[tool(
        description = "Reset a system configuration key to its default value (removes database override). Requires admin.system_config permission."
    )]
    async fn reset_system_config(
        &self,
        Parameters(params): Parameters<ResetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.system_config")?;

        let store =
            self.web_state.system_config.as_ref().ok_or_else(|| {
                McpError::internal_error("system config store not available", None)
            })?;

        store
            .delete(&params.key)
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;

        if let Some(ref ds) = self.web_state.domain_store {
            ds.log_audit(
                &self.auth_user.address,
                "system_config_reset",
                &params.key,
                "",
            )
            .await;
        }

        self.ok_result("reset", &format!("{} reverted to default", params.key))
    }
}
