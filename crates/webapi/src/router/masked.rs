//! Masked addresses: the account's random per-site addresses.

use std::sync::Arc;

use crate::*;

pub(super) fn masked_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::masked_email as m;
    use axum::routing::{get, patch};
    axum::Router::new()
        .route(
            "/api/masked-emails",
            get(m::list_masks).post(m::create_mask),
        )
        .route(
            "/api/masked-emails/{email}",
            patch(m::update_mask).delete(m::delete_mask),
        )
}
