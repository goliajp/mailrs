//! Per-method handler bodies for the pg-core's mailrs-core-api server.
//!
//! Only the SWITCHABLE mail-store families live here (conversations /
//! mailbox / message / thread / admin accounts+aliases+domains). The
//! shared network-kevy side-state families (drafts/signatures/templates/
//! webhooks/audit/contacts/analysis/outbound/groups/api-keys/
//! sieve) are mounted from `mailrs-core-sidestate` so both cores serve
//! them from the same network kevy — see `build_full_router`.

pub mod admin;
pub mod conversation;
pub mod deliver;
pub mod filing;
pub mod holds;
pub mod mailbox;
pub mod message;
pub mod sent;
pub mod thread;
pub mod views;

/// Log a store error for `what` and answer 500.
pub(crate) fn failed<'a>(
    what: &'static str,
    user: &'a str,
) -> impl FnOnce(sqlx::Error) -> axum::http::StatusCode + 'a {
    move |e| {
        tracing::warn!(error = %e, %user, "{what} failed");
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}
