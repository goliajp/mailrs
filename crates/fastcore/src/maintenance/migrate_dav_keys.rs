//! `POST /v1/admin/maintenance:migrate-dav-keys[?delete_old=true]`
//!
//! Moves what the old CalDAV / CardDAV handlers left under
//! `caldav:*:events:*` and `carddav:*:contacts:*` (network kevy) into the
//! event and contact rows the web and CalDAV now share. Idempotent; the
//! answer counts what was walked beside what was written. The old keys
//! stay unless `delete_old=true`, and even then a key holding an entry
//! that would not parse is kept.

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct MigrateDavKeysQuery {
    /// Remove the old keys that moved cleanly.
    #[serde(default)]
    delete_old: bool,
}

pub(crate) async fn migrate_dav_keys_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<MigrateDavKeysQuery>,
) -> axum::response::Response {
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let ran = tokio::task::spawn_blocking(move || {
        mailrs_core_sidestate::families::dav_legacy::migrate(&mut conn, q.delete_old)
    })
    .await;
    match ran {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(err = %e, "dav key migration failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, "dav key migration task panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
