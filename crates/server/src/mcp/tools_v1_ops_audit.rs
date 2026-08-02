//! v1 audit and encryption-key tools.
//!
//! Second half of the `tools_v1_ops` split (2026-08-02).
use super::MailMcpService;
use super::tools::*;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};

#[tool_router(router = tool_router_v1_ops_audit, vis = "pub(crate)")]
impl MailMcpService {
    #[tool(
        description = "Query recent audit log entries. Requires admin.accounts permission. Returns actor, action, target, detail, and timestamp."
    )]
    async fn get_audit_log(
        &self,
        Parameters(params): Parameters<GetAuditLogParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.accounts")?;
        let ds = self.ds()?;
        let limit = params.limit.min(200) as i64;
        let entries = ds.list_audit_log(limit).await.map_err(|e| {
            McpError::internal_error(format!("failed to query audit log: {e}"), None)
        })?;

        let items: Vec<serde_json::Value> = entries
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "timestamp": e.timestamp,
                    "actor": e.actor,
                    "action": e.action,
                    "target": e.target,
                    "detail": e.detail,
                })
            })
            .collect();

        self.json_result(&items)
    }

    #[tool(
        description = "List your encryption keys (PGP and S/MIME). Returns key type, fingerprint, and creation time — not the raw key data."
    )]
    async fn list_own_encryption_keys(
        &self,
        Parameters(_params): Parameters<ListOwnEncryptionKeysParams>,
    ) -> Result<CallToolResult, McpError> {
        let ds = self.ds()?;
        let rows = ds
            .list_encryption_keys(&self.auth_user.address)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let items: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, key_type, fingerprint, created_at)| {
                serde_json::json!({
                    "id": id,
                    "key_type": key_type,
                    "fingerprint": fingerprint,
                    "created_at": created_at,
                })
            })
            .collect();
        self.json_result(&items)
    }

    #[tool(
        description = "Upload or replace your PGP public key or S/MIME certificate. Key type must be 'pgp' or 'smime'."
    )]
    async fn set_encryption_key(
        &self,
        Parameters(params): Parameters<SetEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        if params.public_key.is_empty() {
            return Err(McpError::invalid_params("public_key is required", None));
        }
        if params.public_key.len() > 256 * 1024 {
            return Err(McpError::invalid_params("public_key too large", None));
        }
        let ds = self.ds()?;
        let id = ds
            .set_encryption_key(
                &self.auth_user.address,
                &params.key_type,
                &params.public_key,
                &params.fingerprint,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "status": "key_saved",
                "id": id,
                "key_type": params.key_type,
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Delete your PGP public key or S/MIME certificate. Key type must be 'pgp' or 'smime'."
    )]
    async fn delete_encryption_key(
        &self,
        Parameters(params): Parameters<DeleteEncryptionKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        let ds = self.ds()?;
        let removed = ds
            .delete_encryption_key(&self.auth_user.address, &params.key_type)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        if removed {
            self.ok_result("key_deleted", &params.key_type)
        } else {
            Err(McpError::invalid_params("key not found", None))
        }
    }

    #[tool(
        description = "Look up a recipient's PGP public key or S/MIME certificate by email address. Use this before encrypting an email to someone."
    )]
    async fn get_public_key_of(
        &self,
        Parameters(params): Parameters<GetPublicKeyOfParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.key_type != "pgp" && params.key_type != "smime" {
            return Err(McpError::invalid_params(
                "key_type must be 'pgp' or 'smime'",
                None,
            ));
        }
        if !params.address.contains('@') {
            return Err(McpError::invalid_params("invalid email address", None));
        }
        let ds = self.ds()?;
        match ds
            .get_encryption_key(&params.address, &params.key_type)
            .await
        {
            Ok(Some((_id, public_key, fingerprint))) => {
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::json!({
                        "address": params.address,
                        "key_type": params.key_type,
                        "public_key": public_key,
                        "fingerprint": fingerprint,
                    })
                    .to_string(),
                )]))
            }
            Ok(None) => Err(McpError::invalid_params(
                format!("no {} key found for {}", params.key_type, params.address),
                None,
            )),
            Err(e) => Err(McpError::internal_error(format!("{e}"), None)),
        }
    }

    #[tool(
        description = "List email conversations for a target user (audit/compliance). Requires admin.impersonate permission. Target user must be in your accessible domains."
    )]
    async fn audit_list_conversations(
        &self,
        Parameters(params): Parameters<AuditListConversationsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.impersonate")?;
        self.validate_audit_target(&params.target_user)?;

        let mb = self.mb_store()?;
        let limit = params.limit.unwrap_or(20).min(50);
        let convos = mb
            .list_conversations(
                &params.target_user,
                limit,
                None,
                params.category.as_deref(),
                None,
                false,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        // audit log
        if let Ok(ds) = self.ds() {
            ds.log_audit(
                &self.auth_user.address,
                "audit.list_conversations",
                &params.target_user,
                "",
            )
            .await;
        }

        let items: Vec<serde_json::Value> = convos
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "thread_id": c.thread_id,
                    "subject": c.subject,
                    "participants": c.participants,
                    "message_count": c.message_count,
                    "last_date": c.last_date,
                    "category": c.category,
                    "snippet": c.snippet,
                })
            })
            .collect();

        self.json_result(&items)
    }

    #[tool(
        description = "Read all messages in a thread for a target user (audit/compliance). Requires admin.impersonate permission. Target user must be in your accessible domains."
    )]
    async fn audit_read_thread(
        &self,
        Parameters(params): Parameters<AuditReadThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        self.require_permission("admin.impersonate")?;
        self.validate_audit_target(&params.target_user)?;

        let mb = self.mb_store()?;
        let messages = mb
            .list_thread_messages(&params.target_user, &params.thread_id, None)
            .await
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        // audit log
        if let Ok(ds) = self.ds() {
            ds.log_audit(
                &self.auth_user.address,
                "audit.read_thread",
                &params.target_user,
                &params.thread_id,
            )
            .await;
        }

        let mut items: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
        for msg in &messages {
            let maildir_user = if msg.user_address.is_empty() {
                &params.target_user
            } else {
                &msg.user_address
            };
            let raw = crate::message_util::read_message_raw(
                &self.web_state.maildir_root,
                maildir_user,
                &msg.maildir_id,
            )
            .await;
            let parsed = raw
                .as_deref()
                .map(crate::message_util::parse_message)
                .unwrap_or_default();

            items.push(serde_json::json!({
                "id": msg.id,
                "uid": msg.uid,
                "sender": msg.sender,
                "recipients": msg.recipients,
                "subject": msg.subject,
                "internal_date": msg.internal_date,
                "text_body": parsed.0,
                "attachments": parsed.2.iter().map(|a| serde_json::json!({"filename": a.filename, "content_type": a.content_type, "size": a.size})).collect::<Vec<_>>(),
            }));
        }

        self.json_result(&items)
    }
}
