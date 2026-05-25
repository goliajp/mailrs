use axum::Json;
use axum::response::IntoResponse;

use super::*;

pub(crate) async fn get_all_permissions(AuthUser { .. }: AuthUser) -> impl IntoResponse {
    Json(serde_json::json!(crate::permission::ALL_PERMISSIONS))
}
