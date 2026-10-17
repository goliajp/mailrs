//! Server configuration the admin UI reads and writes: SMTP, the system
//! config map, and the OIDC discovery document.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use crate::handlers::conversations::AuthedUser;

mod catalog;
mod store;
use catalog::{CATALOG, Kind, editable_default, find, refusal, restart_note};
use store::{CONFIG_KEY, CONFIG_META_KEY, ConfigMeta, pairs, remove, store, with_meta};

use crate::handlers::kevy_util::with_kevy;

/// GET /api/auth/oidc/config — OIDC providers list. Empty → login
/// page hides the "Sign in with X" buttons cleanly.
///
/// v2.7.1 §Phase 12 §12.4 (2026-07-13): the pre-fix handler
/// returned `{enabled: false, providers: []}` unconditionally, so
/// the frontend login page never showed the OIDC button on prod
/// even when `MAILRS_OIDC_CLIENT_ID` / `MAILRS_OIDC_CLIENT_SECRET`
/// / `MAILRS_OIDC_ISSUER` were all set. Now mirrors the monolith
/// `web/auth/oidc.rs::oidc_client_config` gating: `enabled` is true
/// iff all three env vars are set, and one provider entry is
/// emitted with `id`, `name` (from `MAILRS_OIDC_PROVIDER_NAME` or
/// `"OIDC"`), and `login_url = /api/auth/oidc/login`.
pub async fn oidc_config() -> Json<serde_json::Value> {
    let enabled = std::env::var("MAILRS_OIDC_CLIENT_ID").is_ok()
        && std::env::var("MAILRS_OIDC_CLIENT_SECRET").is_ok()
        && std::env::var("MAILRS_OIDC_ISSUER").is_ok();
    let providers = if enabled {
        vec![serde_json::json!({
            "id": "primary",
            "name": std::env::var("MAILRS_OIDC_PROVIDER_NAME").unwrap_or_else(|_| "OIDC".into()),
            "login_url": "/api/auth/oidc/login",
        })]
    } else {
        Vec::new()
    };
    Json(serde_json::json!({
        "enabled": enabled,
        "providers": providers,
    }))
}

/// GET /api/admin/system-config
///
/// Returns the `{success, entries}` envelope the admin UI expects. Each
/// entry says where its value comes from and whether it can be changed
/// here (`editable`). Settings read once at process start are shown with
/// the variable and compose service that set them; their `value` is what
/// webapi's own environment says, `null` when it cannot see it. Overrides
/// stored under keys nothing reads are listed too, so they can be reset.
pub async fn get_system_config() -> Result<Json<serde_json::Value>, StatusCode> {
    let (flat, meta_flat) =
        with_kevy(|c| Ok((c.hgetall(CONFIG_KEY)?, c.hgetall(CONFIG_META_KEY)?)))?;
    let overrides = pairs(flat);
    let meta: std::collections::HashMap<String, ConfigMeta> = pairs(meta_flat)
        .into_iter()
        .filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?)))
        .collect();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for setting in CATALOG {
        let mut entry = serde_json::json!({
            "key": setting.key,
            "description": setting.description,
            "group": setting.group,
            "value_type": "string",
        });
        match setting.kind {
            Kind::Boot {
                env_var,
                service,
                default,
            } => {
                let (value, source) = match (default, std::env::var(env_var)) {
                    (None, _) => (None, "unknown"),
                    (Some(_), Ok(v)) => (Some(v), "env"),
                    (Some(d), Err(_)) => (Some(d.to_string()), "default"),
                };
                entry["value"] = serde_json::json!(value);
                entry["default_value"] = serde_json::json!(default);
                entry["source"] = serde_json::json!(source);
                entry["editable"] = serde_json::json!(false);
                entry["env_var"] = serde_json::json!(env_var);
                entry["note"] = serde_json::json!(restart_note(env_var, service));
            }
            Kind::Editable => {
                let default = editable_default(setting.key);
                let stored = overrides.get(setting.key);
                entry["value"] = serde_json::json!(stored.unwrap_or(&default));
                entry["default_value"] = serde_json::json!(default);
                entry["source"] = serde_json::json!(match stored {
                    Some(_) => "database",
                    None => "default",
                });
                entry["editable"] = serde_json::json!(true);
                with_meta(&mut entry, meta.get(setting.key));
            }
        }
        entries.push(entry);
    }
    for (k, v) in &overrides {
        if find(k).is_some() {
            continue;
        }
        let mut entry = serde_json::json!({
            "key": k,
            "value": v,
            "default_value": null,
            "description": "Stored here earlier, but no process reads this key.",
            "group": "custom",
            "source": "database",
            "value_type": "string",
            "editable": false,
            "note": refusal(k, v),
        });
        with_meta(&mut entry, meta.get(k));
        entries.push(entry);
    }
    Ok(Json(serde_json::json!({
        "success": true,
        "entries": entries,
    })))
}

/// The body the admin page sends, and the monolith's `UpdateConfigRequest`.
///
/// This handler used to take a bare `serde_json::Value` and store
/// `body.as_str()` — falling back to the whole document's JSON text when it
/// was not a string. The client sends `{"value": "..."}`, so the fallback
/// was always the branch taken and every setting was stored as the literal
/// `{"value":"..."}`. It never came up because the route was registered for
/// POST while the client sends PUT, so the request was a 405 and never
/// reached here.
#[derive(Debug, serde::Deserialize)]
pub struct SetSystemConfigRequest {
    /// The value to store.
    pub value: String,
}

/// A refused write: the status and `{"error": why}`.
pub type Refused = (StatusCode, Json<serde_json::Value>);

fn save(key: String, value: String, actor: Option<String>) -> Result<StatusCode, Refused> {
    if let Some(why) = refusal(&key, &value) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": why })),
        ));
    }
    store(key, value.trim().to_string(), actor).map_err(|c| {
        (
            c,
            Json(serde_json::json!({ "error": "could not save the setting" })),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// The MCP tool's entry point; the tool writes its own audit row.
pub async fn set_system_config_key(
    Path(k): Path<String>,
    Json(req): Json<SetSystemConfigRequest>,
) -> Result<StatusCode, Refused> {
    save(k, req.value, None)
}

/// `PUT /api/admin/system-config/{key}` from the admin page. Only an
/// editable key is stored; a setting read at process start answers 400
/// with where it is set instead.
pub async fn put_system_config_key(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(k): Path<String>,
    Json(req): Json<SetSystemConfigRequest>,
) -> Result<StatusCode, Refused> {
    let status = save(k.clone(), req.value.clone(), Some(actor.clone()))?;
    crate::handlers::audit::record(&actor, "system_config.set", &k, &req.value);
    Ok(status)
}

/// `DELETE /api/admin/system-config/{key}` — back to the built-in default.
pub async fn delete_system_config_key(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(k): Path<String>,
) -> Result<StatusCode, StatusCode> {
    remove(k.clone())?;
    crate::handlers::audit::record(&actor, "system_config.reset", &k, "");
    Ok(StatusCode::NO_CONTENT)
}
