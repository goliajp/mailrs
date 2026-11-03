//! What the forwarding rules for Sieve scripts make of the scripts already
//! stored: which accounts redirect out of our domains and whether to their
//! verified forwarding address, and how many vacation replies name another
//! sender. Read-only; counts, accounts and domains, never script text.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::FastcoreState;

/// `GET /v1/admin/maintenance:sieve-survey`
pub(crate) async fn sieve_survey_route(State(state): State<Arc<FastcoreState>>) -> Response {
    let done = tokio::task::spawn_blocking(move || {
        let accounts = state.mailbox.list_account_addresses()?;
        let domains = crate::spool_drain::relay::local_domains(&state);
        let mut conn = state
            .net_conn()
            .ok_or_else(|| std::io::Error::other("the network store is unreachable"))?;
        mailrs_intake::sieve_policy::survey(&mut conn, &accounts, &domains, |user, from| {
            crate::routes::may_send_as_blocking(&state, user, from)
        })
    })
    .await;
    match done {
        Ok(Ok(s)) => Json(s).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "sieve survey failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
