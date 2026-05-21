use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use super::*;

// ---------- MTA-STS ----------

pub(crate) async fn mta_sts_policy(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref mode) = state.mta_sts_mode else {
        return (StatusCode::NOT_FOUND, "MTA-STS not configured".to_string());
    };

    let mx_lines: Vec<String> = state
        .mta_sts_mx
        .iter()
        .map(|mx| format!("mx: {mx}"))
        .collect();
    let body = format!(
        "version: STSv1\nmode: {mode}\n{}\nmax_age: {}\nid: {}",
        mx_lines.join("\n"),
        state.mta_sts_max_age,
        state.mta_sts_id
    );

    (StatusCode::OK, body)
}

// ---------- smtp config endpoint ----------

pub(crate) async fn get_smtp_config(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    match &state.smtp_config {
        Some(cfg) => (StatusCode::OK, Json(serde_json::to_value(cfg).unwrap_or_default()))
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "smtp config not available"})),
        )
            .into_response(),
    }
}
