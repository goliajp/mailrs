//! Rules and saved searches — the searches a user keeps and what to do with
//! the mail they find — and the forwarding and vacation reply that also act
//! on mail as it arrives.

use std::sync::Arc;

use crate::*;

pub(super) fn rule_routes() -> axum::Router<Arc<WebState>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/api/rules",
            get(crate::handlers::rules::list).post(crate::handlers::rules::create),
        )
        .route(
            "/api/rules/preview",
            post(crate::handlers::rules_run::preview),
        )
        .route(
            "/api/rules/{id}",
            post(crate::handlers::rules::update).delete(crate::handlers::rules::delete),
        )
        .route(
            "/api/rules/{id}/apply",
            post(crate::handlers::rules_run::apply),
        )
        .route(
            "/api/rules/runs/{run_id}/undo",
            post(crate::handlers::rules_run::undo),
        )
        .route(
            "/api/auth/sessions",
            get(crate::handlers::auth::sessions::list),
        )
        .route(
            "/api/auth/sessions/end-others",
            post(crate::handlers::auth::sessions::end_others),
        )
        .route(
            "/api/auth/sessions/{id}",
            axum::routing::delete(crate::handlers::auth::sessions::end),
        )
        .route(
            "/api/settings/vacation",
            get(crate::handlers::mail_settings::get_vacation)
                .put(crate::handlers::mail_settings::put_vacation),
        )
        .route(
            "/api/settings/forwarding",
            get(crate::handlers::mail_settings::get_forwarding)
                .post(crate::handlers::mail_settings::request_forwarding)
                .delete(crate::handlers::mail_settings::stop_forwarding),
        )
        .route(
            "/api/settings/forwarding/verify",
            post(crate::handlers::mail_settings::verify_forwarding),
        )
        .route(
            "/api/saved-searches",
            get(crate::handlers::saved_searches::list)
                .post(crate::handlers::saved_searches::create),
        )
        .route(
            "/api/saved-searches/{id}",
            axum::routing::delete(crate::handlers::saved_searches::delete),
        )
}
