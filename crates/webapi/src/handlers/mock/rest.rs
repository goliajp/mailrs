//! `/api/mock/*`. The namespace sits in the path so a `mock:{ns}` key is
//! checked by path prefix like every other scope.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::mock::{Filter, MAX_MESSAGES};
use serde::Deserialize;

use super::messages;
use super::service::{self, MockApiError, MockCaller};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::session::AgentScopes;

impl IntoResponse for MockApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

fn caller(user: String, scopes: &AgentScopes) -> MockCaller {
    MockCaller {
        user,
        only_namespaces: scopes.mock_ns_restriction(),
    }
}

type Auth = (Extension<AuthedUser>, Extension<AgentScopes>);

#[derive(Debug, Deserialize)]
pub struct CreateNamespace {
    pub name: String,
}

pub async fn create_namespace(
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Json(body): Json<CreateNamespace>,
) -> Result<impl IntoResponse, MockApiError> {
    let created = service::create_namespace(&caller(user, &scopes), &body.name).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Debug, Deserialize)]
pub struct ListNamespaces {
    #[serde(default)]
    pub all: bool,
}

pub async fn list_namespaces(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Query(q): Query<ListNamespaces>,
) -> Result<impl IntoResponse, MockApiError> {
    let list = service::list_namespaces(&state, &caller(user, &scopes), q.all).await?;
    Ok(Json(serde_json::json!({ "namespaces": list })))
}

pub async fn delete_namespace(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
) -> Result<StatusCode, MockApiError> {
    service::delete_namespace(&state, &caller(user, &scopes), &ns).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default, Deserialize)]
pub struct NewAddress {
    pub tag: Option<String>,
}

pub async fn new_address(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    body: Option<Json<NewAddress>>,
) -> Result<impl IntoResponse, MockApiError> {
    let tag = body.and_then(|Json(b)| b.tag);
    let minted = service::new_address(&state, &caller(user, &scopes), &ns, tag.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(minted)))
}

/// Query of the listing, `latest` and `wait`.
#[derive(Debug, Default, Deserialize)]
pub struct MessageQuery {
    pub tag: Option<String>,
    pub tag_prefix: Option<String>,
    pub to: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
    /// ms since the epoch
    pub since: Option<i64>,
    pub limit: Option<usize>,
    /// seconds, `wait` only
    pub timeout: Option<u64>,
}

impl MessageQuery {
    pub fn filter(&self) -> Filter {
        Filter {
            tag: self.tag.clone(),
            tag_prefix: self.tag_prefix.clone(),
            to: self.to.clone(),
            from: self.from.clone(),
            subject: self.subject.clone(),
            since: self.since,
            limit: self.limit.unwrap_or(50).clamp(1, MAX_MESSAGES),
        }
    }
}

pub async fn list_messages(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    Query(q): Query<MessageQuery>,
) -> Result<impl IntoResponse, MockApiError> {
    let list = messages::list_messages(&state, &caller(user, &scopes), &ns, q.filter()).await?;
    Ok(Json(list))
}

pub async fn latest_message(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    Query(q): Query<MessageQuery>,
) -> Result<impl IntoResponse, MockApiError> {
    match messages::latest(&state, &caller(user, &scopes), &ns, q.filter()).await? {
        Some(m) => Ok(Json(m)),
        None => Err(MockApiError {
            status: StatusCode::NOT_FOUND,
            message: "no message yet".into(),
        }),
    }
}

pub async fn wait_for_message(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    Query(q): Query<MessageQuery>,
) -> Result<impl IntoResponse, MockApiError> {
    let timeout = q.timeout.map(Duration::from_secs);
    let outcome =
        messages::wait_for_message(&state, &caller(user, &scopes), &ns, q.filter(), timeout)
            .await?;
    Ok(Json(outcome))
}

pub async fn read_message(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path((ns, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, MockApiError> {
    Ok(Json(
        messages::read_message(&state, &caller(user, &scopes), &ns, &id).await?,
    ))
}

pub async fn raw_message(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path((ns, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, MockApiError> {
    let raw = messages::read_raw(&state, &caller(user, &scopes), &ns, &id).await?;
    Ok(([(header::CONTENT_TYPE, "message/rfc822")], raw))
}

pub async fn delete_message(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path((ns, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, MockApiError> {
    let removed =
        messages::delete_messages(&state, &caller(user, &scopes), &ns, Some(&id), None).await?;
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

#[derive(Debug, Deserialize)]
pub struct DeleteMessages {
    pub tag: Option<String>,
}

pub async fn delete_messages(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    Query(q): Query<DeleteMessages>,
) -> Result<impl IntoResponse, MockApiError> {
    let removed =
        messages::delete_messages(&state, &caller(user, &scopes), &ns, None, q.tag.as_deref())
            .await?;
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

#[derive(Debug, Deserialize)]
pub struct SendBody {
    pub from_tag: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    #[serde(default)]
    pub text: String,
    pub html: Option<String>,
}

pub async fn send(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
    Json(body): Json<SendBody>,
) -> Result<impl IntoResponse, MockApiError> {
    let req = super::send::SendRequest {
        from_tag: body.from_tag,
        to: body.to,
        subject: body.subject,
        text: body.text,
        html: body.html,
    };
    let sent = super::send::send(&state, &caller(user, &scopes), &ns, req).await?;
    Ok((StatusCode::CREATED, Json(sent)))
}

pub async fn rotate_smtp_token(
    State(state): State<Arc<WebState>>,
    (Extension(AuthedUser(user)), Extension(scopes)): Auth,
    Path(ns): Path<String>,
) -> Result<impl IntoResponse, MockApiError> {
    let creds = messages::rotate_smtp_token(&state, &caller(user, &scopes), &ns).await?;
    Ok((StatusCode::CREATED, Json(creds)))
}
