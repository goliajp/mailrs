//! The ops daemon's administrator routes, passed through to the daemon;
//! merged into the admin surface by `rest.rs`.

use std::sync::Arc;

use crate::*;

pub(super) fn ops_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::admin_ops_daemon as ops;
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/api/admin/ops/state", get(ops::state))
        .route("/api/admin/ops/history", get(ops::history))
        .route("/api/admin/ops/silences", post(ops::set_silence))
        .route("/api/admin/ops/silences/{id}/end", post(ops::end_silence))
        .route("/api/admin/ops/maintenance", post(ops::maintenance))
}
