//! Masked addresses: the account's random per-site addresses.

use std::sync::Arc;

use crate::*;

pub(super) fn masked_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::{forwarders as f, masked_email as m};
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            "/api/masked-emails",
            get(m::list_masks).post(m::create_mask),
        )
        .route(
            "/api/masked-emails/{email}",
            patch(m::update_mask).delete(m::delete_mask),
        )
        // the one alias-creation call of SimpleLogin and of addy.io, which
        // Bitwarden's forwarders send to a server the user names
        .route("/api/alias/random/new", post(f::simplelogin_new))
        .route("/api/v1/aliases", post(f::addy_create))
        .route("/api/admin/masked-submissions", get(m::list_submissions))
}
