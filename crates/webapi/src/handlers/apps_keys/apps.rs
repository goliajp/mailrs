//! Registered apps: a name, a scope list and the sha256 of a secret that is
//! shown once at creation.
//!
//! The record in `admin:apps` is the only place an app's scopes live. No
//! request authenticates with an app secret yet, so the scopes gate nothing
//! until something reads this record.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::complete::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

// the digest stays in kevy; nothing that renders an app needs it
fn public_view(mut app: serde_json::Value) -> serde_json::Value {
    if let Some(o) = app.as_object_mut() {
        o.remove("secret_sha256");
    }
    app
}

fn find_app(
    c: &mut kevy_client::Connection,
    app_id: &str,
) -> std::io::Result<Option<serde_json::Value>> {
    Ok(hgetall_values(c, APPS_KEY)?
        .into_iter()
        .filter_map(|v| serde_json::from_slice::<serde_json::Value>(&v).ok())
        .find(|app| app.get("app_id").and_then(|v| v.as_str()) == Some(app_id)))
}

pub async fn list_apps() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, APPS_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .map(public_view)
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAppRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

pub async fn create_app(
    Json(req): Json<CreateAppRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sha2::{Digest, Sha256};
    let id = with_kevy(|c| next_id(c, APPS_CTR))?;
    let app_id = format!("app_{id}");
    let secret = random_hex(32);
    let secret_sha = format!("{:x}", Sha256::digest(secret.as_bytes()));
    let blob = serde_json::json!({
        "id": id,
        "app_id": app_id,
        "name": req.name,
        "scopes": req.scopes,
        "created_at": now_secs(),
        "secret_sha256": secret_sha,
    });
    let payload = serde_json::to_vec(&blob).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            APPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(())
    })?;
    // the only response that carries the secret; later reads see neither it nor its digest
    Ok(Json(serde_json::json!({
        "id": id,
        "app_id": app_id,
        "secret": secret,
    })))
}

pub async fn get_app(Path(app_id): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    match with_kevy(move |c| find_app(c, &app_id))? {
        Some(app) => Ok(Json(public_view(app))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn delete_app(Path(app_id): Path<String>) -> Result<StatusCode, StatusCode> {
    let removed = with_kevy(move |c| {
        let Some(id) = find_app(c, &app_id)?.and_then(|app| app.get("id").and_then(|v| v.as_i64()))
        else {
            return Ok(false);
        };
        c.hdel(APPS_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(true)
    })?;
    match removed {
        true => Ok(StatusCode::NO_CONTENT),
        false => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct AppScopesRequest {
    pub scopes: Vec<String>,
}

/// PUT /api/admin/apps/{app_id}/scopes — replace the scopes on the app record.
///
/// This once wrote a separate `admin:app:{id}:scopes` string that nothing
/// read, so an edit saved and the record kept its old scopes.
pub async fn set_app_scopes(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(app_id): Path<String>,
    Json(req): Json<AppScopesRequest>,
) -> Result<StatusCode, StatusCode> {
    let joined = req.scopes.join(",");
    let target = app_id.clone();
    let updated = with_kevy(move |c| {
        let Some(mut app) = find_app(c, &target)? else {
            return Ok(false);
        };
        let Some(id) = app.get("id").and_then(|v| v.as_i64()) else {
            return Ok(false);
        };
        app["scopes"] = serde_json::json!(req.scopes);
        let payload = serde_json::to_vec(&app).map_err(std::io::Error::other)?;
        c.hset(
            APPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(true)
    })?;
    if !updated {
        return Err(StatusCode::NOT_FOUND);
    }
    crate::handlers::audit::record(&actor, "app.scopes_update", &app_id, &joined);
    Ok(StatusCode::NO_CONTENT)
}
