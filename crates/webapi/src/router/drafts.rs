//! Drafts and the files kept with them.

use std::sync::Arc;

use crate::*;

pub(super) fn draft_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::{draft_attachments, prefs};
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/api/mail/drafts",
            get(prefs::list_drafts).post(prefs::save_draft),
        )
        .route("/api/mail/drafts/{id}", delete(prefs::delete_draft))
        .route(
            "/api/mail/draft-attachments",
            post(draft_attachments::upload),
        )
        .route(
            "/api/mail/draft-attachments/{blob_id}",
            get(draft_attachments::download),
        )
}
