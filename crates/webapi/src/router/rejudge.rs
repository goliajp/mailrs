//! What re-judging took back, and the super administrator's decisions on
//! it, merged into the admin surface by `rest.rs`.

use std::sync::Arc;

use crate::*;

pub(super) fn rejudge_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::rejudge;
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/api/admin/rejudge/batches", get(rejudge::batches))
        .route(
            "/api/admin/fraud/rules/{rule}/triggers",
            get(rejudge::rule_triggers),
        )
        .route(
            "/api/admin/rejudge/batches/{observation}/release",
            post(rejudge::release),
        )
        .route(
            "/api/admin/rejudge/batches/{observation}/confirm",
            post(rejudge::confirm),
        )
        .route(
            "/api/admin/rejudge/batches/{observation}/discard",
            post(rejudge::discard),
        )
}
