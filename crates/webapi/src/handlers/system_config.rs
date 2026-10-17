//! Server configuration the admin UI reads and writes: SMTP, the system
//! config map, and the OIDC discovery document.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use crate::handlers::conversations::AuthedUser;

mod store;
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
/// Returns the `{success, entries}` envelope the admin UI expects.
/// Each entry describes a single tunable — its current value, where
/// the value came from (env / database / default), and enough metadata
/// for the UI to render an editor.
///
/// The fastcore lane treats runtime tuning as a small collection of
/// well-known keys rather than a fully-dynamic catalog. If the operator
/// has overridden a key via `POST /api/admin/system-config/{k}`, it's
/// read from kevy; otherwise the `source: "env"` reading (or the built-
/// in default) wins. UI renders "Environment" pill next to the value.
pub async fn get_system_config() -> Result<Json<serde_json::Value>, StatusCode> {
    let (flat, meta_flat) =
        with_kevy(|c| Ok((c.hgetall(CONFIG_KEY)?, c.hgetall(CONFIG_META_KEY)?)))?;
    let overrides = pairs(flat);
    let meta: std::collections::HashMap<String, ConfigMeta> = pairs(meta_flat)
        .into_iter()
        .filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?)))
        .collect();
    // The catalog is the union of what the operator has already
    // overridden and a small built-in list of tunables the UI wants to
    // surface even on a fresh install. Keeps the page useful when
    // kevy has no override rows yet.
    const CATALOG: &[(&str, &str, &str, &str, &str)] = &[
        // (key, group, description, env_var, default)
        (
            "hostname",
            "smtp",
            "Public SMTP hostname (HELO / greeting)",
            "MAILRS_HOSTNAME",
            "",
        ),
        (
            "smtp_port",
            "smtp",
            "Inbound SMTP port on the receiver process",
            "MAILRS_SMTP_PORT",
            "25",
        ),
        (
            "submission_port",
            "smtp",
            "Authenticated submission port",
            "MAILRS_SUBMISSION_PORT",
            "587",
        ),
        (
            "imap_port",
            "imap",
            "IMAP port on the fastcore process",
            "MAILRS_IMAP_PORT",
            "143",
        ),
        (
            "local_domains",
            "smtp",
            "Comma-separated list of accepted local domains",
            "MAILRS_LOCAL_DOMAINS",
            "",
        ),
        (
            "tls_enabled",
            "security",
            "Serve STARTTLS / IMAPS with certificates",
            "MAILRS_TLS_ENABLED",
            "true",
        ),
        (
            "max_message_size_bytes",
            "smtp",
            "Reject inbound mail larger than this (bytes)",
            "MAILRS_MAX_MESSAGE_SIZE_BYTES",
            "",
        ),
    ];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for (key, group, description, env_var, default) in CATALOG {
        seen.insert(key.to_string());
        let (value, source) = if let Some(v) = overrides.get(*key) {
            (v.clone(), "database")
        } else if let Ok(v) = std::env::var(env_var) {
            (v, "env")
        } else {
            (default.to_string(), "default")
        };
        let mut entry = serde_json::json!({
            "key": key,
            "value": value,
            "default_value": default,
            "description": description,
            "group": group,
            "source": source,
            "value_type": "string",
        });
        if source == "database" {
            with_meta(&mut entry, meta.get(*key));
        }
        entries.push(entry);
    }
    // Any operator override that isn't in the built-in catalog still
    // gets surfaced so the UI can show / edit / remove it.
    for (k, v) in &overrides {
        if seen.contains(k) {
            continue;
        }
        let mut entry = serde_json::json!({
            "key": k,
            "value": v,
            "default_value": "",
            "description": "Operator-defined key (no built-in metadata).",
            "group": "custom",
            "source": "database",
            "value_type": "string",
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

/// The MCP tool's entry point; the tool writes its own audit row.
pub async fn set_system_config_key(
    Path(k): Path<String>,
    Json(req): Json<SetSystemConfigRequest>,
) -> Result<StatusCode, StatusCode> {
    store(k, req.value, None)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /api/admin/system-config/{key}` from the admin page.
pub async fn put_system_config_key(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(k): Path<String>,
    Json(req): Json<SetSystemConfigRequest>,
) -> Result<StatusCode, StatusCode> {
    store(k.clone(), req.value.clone(), Some(actor.clone()))?;
    crate::handlers::audit::record(&actor, "system_config.set", &k, &req.value);
    Ok(StatusCode::NO_CONTENT)
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
