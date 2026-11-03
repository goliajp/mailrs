//! Team inboxes: a team conversation's state, assignment, presence, notes
//! and the reply being written.

use std::sync::Arc;

use crate::*;

pub(super) fn shared_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::shared_inbox::{self as t, drafts, notes};
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/api/shared/{gid}/{conv}", get(t::overview))
        .route("/api/shared/{gid}/{conv}/assign", post(t::assign_conv))
        .route(
            "/api/shared/{gid}/{conv}/presence",
            post(t::heartbeat).delete(t::leave),
        )
        .route(
            "/api/shared/{gid}/{conv}/notes",
            get(notes::list).post(notes::add),
        )
        .route(
            "/api/shared/{gid}/{conv}/notes/{id}",
            put(notes::edit).delete(notes::delete),
        )
        .route(
            "/api/shared/{gid}/{conv}/draft",
            get(drafts::current)
                .put(drafts::announce)
                .delete(drafts::clear),
        )
        .route(
            "/api/shared/{gid}/{conv}/draft/take-over",
            post(drafts::take_over),
        )
        .route(
            "/api/admin/shared/{gid}/{conv}/deleted-notes/{id}",
            get(notes::deleted_original),
        )
        .route(
            "/api/admin/email-groups/{id}/shared",
            put(t::set_group_shared),
        )
}
