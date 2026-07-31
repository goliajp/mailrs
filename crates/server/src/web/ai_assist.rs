//! The three writing-assistance routes, as an adapter.
//!
//! The request shapes, sanitising and prompts moved to
//! `mailrs_intelligence::assist` when the fastcore lane needed them too.
//! Copying the file across would have left two implementations of one
//! contract with nothing to notice them drifting — the failure class this
//! tree keeps paying for. What stays here is the extractor and the response
//! type; everything a test would want to assert lives in the stone.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use mailrs_intelligence::assist::{
    self, AssistUnavailable, PolishRequest, PolishResult, ReplySuggestRequest, ReplySuggestResult,
    SubjectGenerateRequest, SubjectGenerateResult,
};

use super::{AuthUser, WebState};

pub(super) async fn ai_polish(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<PolishRequest>,
) -> impl IntoResponse {
    let Some(ref provider) = state.llm_config else {
        return Json(PolishResult::failed(AssistUnavailable::NotConfigured));
    };
    Json(assist::polish(provider.as_ref(), &req).await)
}

pub(super) async fn ai_reply_suggest(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<ReplySuggestRequest>,
) -> impl IntoResponse {
    let Some(ref provider) = state.llm_config else {
        return Json(ReplySuggestResult::failed(AssistUnavailable::NotConfigured));
    };
    Json(assist::reply_suggest(provider.as_ref(), &req).await)
}

pub(super) async fn ai_generate_subject(
    AuthUser { .. }: AuthUser,
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
