//! OAuth client registration — the admin CRUD behind /admin/oauth-clients.

use axum::Json;
use axum::extract::Extension;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OauthClient {
    pub client_id: String,
    pub name: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateOauthClientRequest {
    pub name: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateOauthClientResponse {
    pub client_id: String,
    pub client_secret: String,
    pub name: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

/// GET /api/admin/oauth-clients
pub async fn list_oauth_clients(
    Extension(_user): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let members = with_kevy(|c| {
        c.smembers(b"oidc:clients:index")
            .map_err(std::io::Error::other)
    })
    .unwrap_or_default();
    let mut items = Vec::new();
    for m in members {
        if let Ok(cid) = String::from_utf8(m) {
            let key = format!("oidc:client:{cid}");
            let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))
                .unwrap_or_default();
            let mut fields = std::collections::HashMap::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                fields.insert(
                    String::from_utf8_lossy(&flat[i]).to_string(),
                    String::from_utf8_lossy(&flat[i + 1]).to_string(),
                );
                i += 2;
            }
            items.push(OauthClient {
                client_id: cid,
                name: fields.get("name").cloned().unwrap_or_default(),
                redirect_uri: fields.get("redirect_uri").cloned().unwrap_or_default(),
                scopes: fields
                    .get("scopes")
                    .cloned()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                created_at: fields
                    .get("created_at")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            });
        }
    }
    Json(serde_json::json!({ "items": items }))
}

/// POST /api/admin/oauth-clients
pub async fn create_oauth_client(
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<CreateOauthClientRequest>,
) -> Json<CreateOauthClientResponse> {
    let client_id = format!("mail-{}", random_hex(6));
    let client_secret = random_hex(32);
    let key = format!("oidc:client:{client_id}");
    let scopes_csv = req.scopes.join(",");
    let now = now_secs();
    let cid_c = client_id.clone();
    let secret_c = client_secret.clone();
    let name_c = req.name.clone();
    let ru_c = req.redirect_uri.clone();
    let scopes_c = scopes_csv.clone();
    let _ = with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"name" as &[u8], name_c.as_bytes()),
                (b"redirect_uri", ru_c.as_bytes()),
                (b"secret", secret_c.as_bytes()),
                (b"scopes", scopes_c.as_bytes()),
                (b"created_at", now.to_string().as_bytes()),
            ],
        )
        .map_err(std::io::Error::other)?;
        c.sadd(b"oidc:clients:index", &[cid_c.as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    });
    Json(CreateOauthClientResponse {
        client_id,
        client_secret,
        name: req.name,
        redirect_uri: req.redirect_uri,
        scopes: req.scopes,
    })
}

/// DELETE /api/admin/oauth-clients/{client_id}
pub async fn delete_oauth_client(
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> StatusCode {
    let key = format!("oidc:client:{client_id}");
    let cid_c = client_id.clone();
    let _ = with_kevy(move |c| {
        c.del(&[key.as_bytes()]).map_err(std::io::Error::other)?;
        c.srem(b"oidc:clients:index", &[cid_c.as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

// ── helpers ──────────────────────────────────────────────────────
