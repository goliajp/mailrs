use std::sync::Arc;

use crate::*;

/// `/api/mock/*`: disposable test mailboxes.
pub(super) fn mock_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::mock::rest as mock;
    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        .route(
            "/api/mock/namespaces",
            get(mock::list_namespaces).post(mock::create_namespace),
        )
        .route("/api/mock/namespaces/{ns}", delete(mock::delete_namespace))
        .route(
            "/api/mock/namespaces/{ns}/addresses",
            post(mock::new_address),
        )
        .route("/api/mock/namespaces/{ns}/send", post(mock::send))
        .route(
            "/api/mock/namespaces/{ns}/webhook",
            put(mock::set_webhook).delete(mock::clear_webhook),
        )
        .route(
            "/api/mock/namespaces/{ns}/smtp-token",
            post(mock::rotate_smtp_token),
        )
        .route(
            "/api/mock/namespaces/{ns}/messages",
            get(mock::list_messages).delete(mock::delete_messages),
        )
        .route(
            "/api/mock/namespaces/{ns}/messages/latest",
            get(mock::latest_message),
        )
        .route(
            "/api/mock/namespaces/{ns}/messages/wait",
            get(mock::wait_for_message),
        )
        .route(
            "/api/mock/namespaces/{ns}/messages/{id}",
            get(mock::read_message).delete(mock::delete_message),
        )
        .route(
            "/api/mock/namespaces/{ns}/messages/{id}/raw",
            get(mock::raw_message),
        )
}
