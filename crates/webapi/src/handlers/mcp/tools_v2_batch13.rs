//! v2 MCP tool batch 13 — greylist local rules, the caller's own
//! encryption keys, and system config. Two-lane parity with the
//! monolith's `greylist_local_add` / `greylist_local_remove` /
//! `set_encryption_key` / `delete_encryption_key` /
//! `get_system_config` / `set_system_config` / `reset_system_config`.
//!
//! Greylist + system-config delegate to `handlers::complete`, keys to
//! `handlers::keys` — the same handlers the admin UI drives. Only
//! `reset_system_config` has no REST twin (the fastcore lane exposes
//! no DELETE route), so it drops the override field directly.

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

use super::params::{
    DeleteEncryptionKeyParams, GreylistLocalAddParams, GreylistLocalRemoveParams,
    SetEncryptionKeyParams, SetSystemConfigParams, SystemConfigKeyParams,
};
use super::{MailrsMcpService, ok_result};
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn map_status(code: StatusCode, what: &str) -> McpError {
    if code == StatusCode::NOT_FOUND {
        return McpError::invalid_params(format!("{what}: not found"), None);
    }
    McpError::internal_error(format!("{what}: {code}"), None)
}

/// Unwrap the `{success, message}` envelope the keys handlers return
/// so an MCP caller sees a failure as an error, not as a success body.
fn unwrap_key_result(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    if v.get("success").and_then(|s| s.as_bool()) == Some(true) {
        return Ok(ok_result());
    }
    let msg = v
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("key operation failed")
        .to_string();
    Err(McpError::invalid_params(msg, None))
}

#[tool_router(router = tool_router_v2_batch13, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "Add a local greylist rule. `white` entries bypass the greylist triplet check; `black` entries are rejected (`whitelist` / `blacklist` are accepted too). Admin-gated."
    )]
    async fn greylist_local_add(
        &self,
        Parameters(params): Parameters<GreylistLocalAddParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::handlers::complete::{
            NewGreylistEntry, insert_entry, kind_of_value, normalise_list,
        };
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let Some(list) = normalise_list(&params.list_type) else {
            return Err(McpError::invalid_params(
                "list_type must be 'white' or 'black'",
                None,
            ));
        };
        // the parameter names are a contract with the agents that call the
        // tool; the stored words are the admin page's, so both read alike
        let value = params.address_or_domain.trim();
        let entry = with_kevy(|c| {
            insert_entry(
                c,
                &NewGreylistEntry {
                    kind: kind_of_value(value),
                    list,
                    value,
                    note: None,
                    created_by: &user,
                },
            )
        })
        .map_err(|c| map_status(c, "greylist_local_add"))?;
        crate::handlers::audit::record(&user, "greylist.add", value, list);
        Ok(CallToolResult::success(vec![Content::text(
            entry.to_string(),
        )]))
    }

    #[tool(description = "Remove a local greylist rule by id. Admin-gated.")]
    async fn greylist_local_remove(
        &self,
        Parameters(params): Parameters<GreylistLocalRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        crate::handlers::complete::delete_greylist_entry(Path(params.id.to_string()))
            .await
            .map_err(|c| map_status(c, "greylist_local_remove"))?;
        crate::handlers::audit::record(&user, "greylist.remove", &params.id.to_string(), "via mcp");
        Ok(ok_result())
    }

    #[tool(
        description = "Upload or replace the caller's own PGP public key or S/MIME certificate. key_type must be 'pgp' or 'smime'."
    )]
    async fn set_encryption_key(
        &self,
        Parameters(params): Parameters<SetEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let req = crate::handlers::keys::SetKeyRequest {
            public_key: params.public_key,
            fingerprint: params.fingerprint,
        };
        let Json(res) = crate::handlers::keys::set_key(
            axum::Extension(AuthedUser(user)),
            Path(params.key_type),
            Json(req),
        )
        .await;
        unwrap_key_result(res)
    }

    #[tool(
        description = "Delete the caller's own PGP public key or S/MIME certificate. key_type must be 'pgp' or 'smime'."
    )]
    async fn delete_encryption_key(
        &self,
        Parameters(params): Parameters<DeleteEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let Json(res) = crate::handlers::keys::delete_key(
            axum::Extension(AuthedUser(user)),
            Path(params.key_type),
        )
        .await;
        unwrap_key_result(res)
    }

    #[tool(
        description = "List every system configuration entry with its current value, group, and source (database override / env / built-in default). Admin-gated."
    )]
    async fn get_system_config(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let Json(cfg) = crate::handlers::complete::get_system_config()
            .await
            .map_err(|c| map_status(c, "get_system_config"))?;
        Ok(CallToolResult::success(vec![Content::text(
            cfg.to_string(),
        )]))
    }

    #[tool(
        description = "Set a system configuration value as a database override. Use `get_system_config` for the key list. Admin-gated."
    )]
    async fn set_system_config(
        &self,
        Parameters(params): Parameters<SetSystemConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        crate::handlers::complete::set_system_config_key(
            Path(params.key.clone()),
            Json(crate::handlers::complete::SetSystemConfigRequest {
                value: params.value.clone(),
            }),
        )
        .await
        .map_err(|(code, Json(body))| match code {
            StatusCode::BAD_REQUEST => McpError::invalid_params(
                body["error"].as_str().unwrap_or("refused").to_string(),
                None,
            ),
            _ => map_status(code, "set_system_config"),
        })?;
        crate::handlers::audit::record(&user, "system_config.set", &params.key, &params.value);
        Ok(ok_result())
    }

    #[tool(
        description = "Drop a system configuration override so the key falls back to its env / built-in default. Admin-gated."
    )]
    async fn reset_system_config(
        &self,
        Parameters(params): Parameters<SystemConfigKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        self.require_admin(&user).await?;
        let key = params.key.clone();
        with_kevy(move |c| {
            c.hdel(b"admin:system-config", &[key.as_bytes()])?;
            Ok(())
        })
        .map_err(|_| McpError::internal_error("system config reset failed", None))?;
        crate::handlers::audit::record(&user, "system_config.reset", &params.key, "via mcp");
        Ok(ok_result())
    }
}
