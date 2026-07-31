//! The three writing-assistance routes.
//!
//! Adapters only: the request shapes, sanitising and prompts live in
//! `mailrs_intelligence::assist`, shared with the monolith lane. Before this
//! module existed the fastcore lane — the one production runs — had no route
//! for any of them, so Polish, Suggest and Generate subject answered 405 and
//! the client showed a generic failure. With no model configured they now
//! answer that, in words.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::response::IntoResponse;
use mailrs_intelligence::assist::{
    self, AssistUnavailable, PolishRequest, PolishResult, ReplySuggestRequest, ReplySuggestResult,
    SubjectGenerateRequest, SubjectGenerateResult,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// `POST /api/mail/ai/polish`
pub async fn ai_polish(
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    State(state): State<Arc<WebState>>,
    Json(req): Json<PolishRequest>,
) -> impl IntoResponse {
    let Some(ref provider) = state.llm_config else {
        return Json(PolishResult::failed(AssistUnavailable::NotConfigured));
    };
    Json(assist::polish(provider.as_ref(), &req).await)
}

/// `POST /api/mail/ai/reply-suggest`
pub async fn ai_reply_suggest(
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    State(state): State<Arc<WebState>>,
    Json(req): Json<ReplySuggestRequest>,
) -> impl IntoResponse {
    let Some(ref provider) = state.llm_config else {
        return Json(ReplySuggestResult::failed(AssistUnavailable::NotConfigured));
    };
    Json(assist::reply_suggest(provider.as_ref(), &req).await)
}

/// `POST /api/mail/ai/generate-subject`
pub async fn ai_generate_subject(
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SubjectGenerateRequest>,
) -> impl IntoResponse {
    let Some(ref provider) = state.llm_config else {
        return Json(SubjectGenerateResult::failed(
            AssistUnavailable::NotConfigured,
        ));
    };
    Json(assist::generate_subject(provider.as_ref(), &req).await)
}
