//! A reader's rules for senders and their push settings: correcting a
//! sender once and having it remembered, keeping a sender's mail for a
//! while, and when mail rings their devices.

use std::sync::Arc;

use crate::*;

pub(super) fn sender_and_notify_routes() -> axum::Router<Arc<WebState>> {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route(
            "/api/senders/reclassify",
            post(crate::handlers::sender_rules::reclassify),
        )
        .route(
            "/api/senders/corrections/{id}/undo",
            post(crate::handlers::sender_rules::undo),
        )
        .route(
            "/api/sender-rules",
            get(crate::handlers::sender_rules::list),
        )
        .route(
            "/api/sender-rules/{address}",
            put(crate::handlers::sender_rules::put).delete(crate::handlers::sender_rules::delete),
        )
        .route(
            "/api/notify",
            get(crate::handlers::notify::get_prefs).put(crate::handlers::notify::put_prefs),
        )
        .route(
            "/api/notify/overrides",
            get(crate::handlers::notify::overrides),
        )
        .route(
            "/api/notify/senders/{address}",
            put(crate::handlers::notify::put_sender),
        )
        .route(
            "/api/notify/threads/{thread_id}",
            put(crate::handlers::notify::put_thread),
        )
        .route("/api/notify/stats", get(crate::handlers::notify::stats))
        .route(
            "/api/contacts/important",
            get(crate::handlers::notify::important),
        )
        .route(
            "/api/contacts/important/{address}",
            put(crate::handlers::notify::put_important),
        )
        .route("/api/push/web", get(crate::handlers::web_push::state))
        .route(
            "/api/push/web-subscriptions",
            post(crate::handlers::web_push::subscribe)
                .delete(crate::handlers::web_push::unsubscribe),
        )
        .route(
            "/api/push/clicked",
            post(crate::handlers::web_push::clicked),
        )
}
