//! The signed-in user's saved searches. A name or query the rules refuse is
//! answered 400 with the reason; a taken name 409.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::saved_searches::{self, Refused};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// GET /api/saved-searches — by name.
pub async fn list(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<saved_searches::SavedSearch>>, StatusCode> {
    let items = with_kevy(move |c| saved_searches::list(c, &user))?;
    Ok(Json(items))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateRequest {
    pub name: String,
    pub query: String,
}

/// POST /api/saved-searches `{name, query}`.
pub async fn create(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateRequest>,
) -> Response {
    let now = chrono::Utc::now().timestamp();
    let saved = with_kevy(move |c| saved_searches::create(c, &user, &req.name, &req.query, now));
    match saved {
        Ok(Ok(item)) => (StatusCode::CREATED, Json(item)).into_response(),
        Ok(Err(why)) => {
            let status = match why {
                Refused::Duplicate => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            (status, Json(serde_json::json!({ "error": why.code() }))).into_response()
        }
        Err(status) => status.into_response(),
    }
}

/// DELETE /api/saved-searches/{id}
pub async fn delete(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> StatusCode {
    match with_kevy(move |c| saved_searches::delete(c, &user, &id)) {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(status) => status,
    }
}
