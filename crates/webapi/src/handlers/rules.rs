//! The signed-in user's rules: a search, and what to do with the mail it
//! finds. Kept here; both cores run them on mail as it arrives. Running one
//! over mail already received, and undoing that, is `rules_run`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::rules::{self as stored, Draft, Rule};
use mailrs_mailbox::rules::{Action, check};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, serde::Deserialize)]
pub struct RuleRequest {
    pub name: String,
    pub query: String,
    pub actions: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

pub(crate) fn refused(code: &str, status: StatusCode) -> Response {
    (status, Json(serde_json::json!({ "error": code }))).into_response()
}

/// GET /api/rules — in the order they run.
pub async fn list(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<Rule>>, StatusCode> {
    Ok(Json(with_kevy(move |c| stored::list(c, &user))?))
}

/// The actions, checked against the query rules and the user's labels;
/// stored spellings on success.
async fn checked(state: &WebState, user: &str, req: &RuleRequest) -> Result<Vec<String>, Fail> {
    let actions =
        check(&req.query, &req.actions).map_err(|why| (StatusCode::BAD_REQUEST, why.code()))?;
    let wants_labels = actions.iter().any(|a| matches!(a, Action::Label(_)));
    if wants_labels {
        let labels = state
            .core
            .list_labels(user)
            .await
            .map_err(|_| (StatusCode::BAD_GATEWAY, "labels_unavailable"))?;
        let known = |id: &str| labels.iter().any(|l| l.id == id);
        let unknown = actions.iter().any(|a| match a {
            Action::Label(id) => !known(id),
            _ => false,
        });
        if unknown {
            return Err((StatusCode::BAD_REQUEST, "invalid_action"));
        }
    }
    Ok(actions.iter().map(Action::spelling).collect())
}

/// A refusal: the status and the error code the API answers with.
pub(crate) type Fail = (StatusCode, &'static str);

/// Check and store a rule: a new one when `id` is `None`.
pub(crate) async fn save(
    state: &WebState,
    user: &str,
    id: Option<String>,
    req: RuleRequest,
) -> Result<Rule, Fail> {
    let actions = checked(state, user, &req).await?;
    let now = chrono::Utc::now().timestamp();
    let who = user.to_string();
    let saved = with_kevy(move |c| {
        let draft = Draft {
            name: &req.name,
            query: &req.query,
            actions,
            enabled: req.enabled,
        };
        match id {
            None => stored::create(c, &who, draft, now),
            Some(id) => stored::update(c, &who, &id, draft),
        }
    })
    .map_err(|status| (status, "store_failed"))?;
    saved.map_err(|why| {
        let status = match why {
            stored::Refused::Duplicate => StatusCode::CONFLICT,
            stored::Refused::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, why.code())
    })
}

fn answer(saved: Result<Rule, Fail>, created: StatusCode) -> Response {
    match saved {
        Ok(rule) => (created, Json(rule)).into_response(),
        Err((status, code)) => refused(code, status),
    }
}

/// POST /api/rules `{name, query, actions, enabled?}`.
pub async fn create(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<RuleRequest>,
) -> Response {
    answer(save(&state, &user, None, req).await, StatusCode::CREATED)
}

/// POST /api/rules/{id} — replace a rule's name, query, actions and switch.
pub async fn update(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<RuleRequest>,
) -> Response {
    answer(save(&state, &user, Some(id), req).await, StatusCode::OK)
}

/// DELETE /api/rules/{id}
pub async fn delete(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> StatusCode {
    match with_kevy(move |c| stored::delete(c, &user, &id)) {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(status) => status,
    }
}
