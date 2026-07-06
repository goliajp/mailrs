//! v2.0.0 MCP tool batch 5 — encryption-key introspection tools.
//!
//! `encryption_keys:{user}` hash stores per-key-type (`pgp`, `smime`)
//! entries as JSON. list_own_encryption_keys returns fingerprints
//! without the private material; get_public_key_of returns a
//! recipient's PUBLIC key so an agent can encrypt to that recipient
//! without needing a separate keyserver.

use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::MailrsMcpService;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecipientAddressParams {
    /// Email address to look up a public key for.
    pub address: String,
    /// Which key type: `pgp` or `smime`. Case-insensitive.
    pub key_type: String,
}

#[derive(Debug, serde::Deserialize)]
struct StoredKeyLite {
    fingerprint: Option<String>,
    created_at: Option<i64>,
    public_key: Option<String>,
}

#[tool_router(router = tool_router_v2_batch5, vis = "pub")]
impl MailrsMcpService {
    #[tool(
        description = "List the caller's own encryption keys (PGP + S/MIME). Returns { key_type, fingerprint, created_at } per entry. Public keys and private material are NOT returned — this is a listing / fingerprint check only."
    )]
    async fn list_own_encryption_keys(&self) -> Result<CallToolResult, McpError> {
        let user = self.require_user()?.to_string();
        let key = format!("encryption_keys:{user}");
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))
            .map_err(|_| McpError::internal_error("kevy read", None))?;
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < flat.len() {
            let key_type = String::from_utf8_lossy(&flat[i]).to_string();
            if let Ok(stored) = serde_json::from_slice::<StoredKeyLite>(&flat[i + 1]) {
                out.push(serde_json::json!({
                    "key_type": key_type,
                    "fingerprint": stored.fingerprint,
                    "created_at": stored.created_at,
                }));
            }
            i += 2;
        }
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({ "items": out }).to_string(),
        )]))
    }

    #[tool(
        description = "Fetch the public encryption key of another address so the agent can encrypt outbound mail to them. Returns { address, key_type, public_key, fingerprint }. Returns { ok: false } when the recipient has no key of the requested type."
    )]
    async fn get_public_key_of(
        &self,
        Parameters(params): Parameters<RecipientAddressParams>,
    ) -> Result<CallToolResult, McpError> {
        // Public-key lookups are inherently non-secret — no admin
        // gate. Caller must be authenticated though.
        let _ = self.require_user()?;
        let key_type = params.key_type.to_lowercase();
        if key_type != "pgp" && key_type != "smime" {
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({ "ok": false, "reason": "key_type must be 'pgp' or 'smime'" })
                    .to_string(),
            )]));
        }
        let hkey = format!("encryption_keys:{}", params.address);
        let kt = key_type.clone();
        let value = with_kevy(move |c| c.hget(hkey.as_bytes(), kt.as_bytes()))
            .map_err(|_| McpError::internal_error("kevy read", None))?;
        let Some(bytes) = value else {
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({ "ok": false, "reason": "no key for that (address, key_type)" })
                    .to_string(),
            )]));
        };
        let Ok(stored) = serde_json::from_slice::<StoredKeyLite>(&bytes) else {
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::json!({ "ok": false, "reason": "stored key is malformed" }).to_string(),
            )]));
        };
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::json!({
                "ok": true,
                "address": params.address,
                "key_type": key_type,
                "public_key": stored.public_key,
                "fingerprint": stored.fingerprint,
            })
            .to_string(),
        )]))
    }
}
