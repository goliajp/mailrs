//! OAuth client registration — the admin CRUD behind /admin/oauth-clients.

use axum::Json;
use axum::extract::Extension;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

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

/// GET /api/admin/oauth-clients
pub async fn list_oauth_clients(
    Extension(_user): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let members = with_kevy(|c| {
        c.smembers(b"oidc:clients:index")
            .map_err(std::io::Error::from)
    })
    .unwrap_or_default();
    let mut items = Vec::new();
    for m in members {
        if let Ok(cid) = String::from_utf8(m) {
            let key = format!("oidc:client:{cid}");
            let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))
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

/// POST /api/admin/oauth-clients — refused.
///
/// `/oauth/authorize` issues a code for the signed-in user without asking
/// them to approve the client. With a client registered, any page the user
/// visits could send them through it and come back holding their identity.
/// Registration stays closed until that approval step exists, so a client
/// cannot be connected to the gap first.
pub async fn create_oauth_client(
    Extension(_user): Extension<AuthedUser>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "oauth_client_registration_closed",
            "detail": "Registering OAuth clients is closed until /oauth/authorize asks the user to approve each client.",
        })),
    )
        .into_response()
}

/// DELETE /api/admin/oauth-clients/{client_id}
pub async fn delete_oauth_client(
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> StatusCode {
    let key = format!("oidc:client:{client_id}");
    let cid_c = client_id.clone();
    let _ = with_kevy(move |c| {
        c.del(&[key.as_bytes()])?;
        c.srem(b"oidc:clients:index", &[cid_c.as_bytes()])?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

// ── helpers ──────────────────────────────────────────────────────
