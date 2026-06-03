//! BIMI logo lookup endpoint (with 24h Kevy cache).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::WebState;

/// look up BIMI logo URL for a domain (cached in Kevy for 24h)
pub(crate) async fn get_bimi_logo(
    Path(domain): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    // validate domain
    if domain.len() > 253 || domain.contains('/') || !domain.contains('.') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid domain"})),
        );
    }

    // check kevy cache
    let cache_key = format!("bimi:{domain}");
    if let Some(ref store) = state.kevy_embed
        && let Ok(Some(bytes)) = store.get(cache_key.as_bytes())
        && let Ok(cached) = String::from_utf8(bytes)
    {
        if cached.is_empty() {
            return (StatusCode::OK, Json(serde_json::json!({"logo_url": null})));
        }
        return (
            StatusCode::OK,
            Json(serde_json::json!({"logo_url": cached})),
        );
    }

    // dns lookup
    let Some(ref resolver) = state.resolver else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "DNS resolver not available"})),
        );
    };
    let pm_resolver = mailrs_postmaster::HickoryPostmasterResolver::new((**resolver).clone());
    let logo_url = mailrs_postmaster::lookup_bimi_logo(&pm_resolver, &domain).await;

    // cache result (24h), empty string = negative cache
    if let Some(ref store) = state.kevy_embed {
        let val = logo_url.as_deref().unwrap_or("");
        let _ = store.set_with_ttl(
            cache_key.as_bytes(),
            val.as_bytes(),
            std::time::Duration::from_secs(86400),
        );
    }

    match logo_url {
        Some(url) => (StatusCode::OK, Json(serde_json::json!({"logo_url": url}))),
        None => (StatusCode::OK, Json(serde_json::json!({"logo_url": null}))),
    }
}
