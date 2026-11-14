//! Operator routes of this core that read the store and the network kevy
//! the way fastcore's do.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::core_rpc::CoreRpcState;

/// `GET /v1/admin/maintenance:sieve-survey` — which stored Sieve scripts
/// the forwarding rules change: counts, accounts and domains, never script
/// text. Each account's permissions are read before the scripts, so the
/// survey's send-as test is the one sending applies.
pub async fn sieve_survey(State(state): State<Arc<CoreRpcState>>) -> Response {
    let fail = |e: String| {
        tracing::error!(error = %e, "sieve survey failed");
        (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
    };
    let accounts: Vec<String> = match state.domain.list_accounts().await {
        Ok(a) => a.into_iter().map(|a| a.address).collect(),
        Err(e) => return fail(e.to_string()),
    };
    let names = match state.domain.domain_names().await {
        Ok(n) => n,
        Err(e) => return fail(e.to_string()),
    };
    let domains = mailrs_intake::domains::LocalDomainsCache::default()
        .get(move || Ok(names))
        .to_vec();
    let mut perms = HashMap::new();
    for user in &accounts {
        let p =
            super::admin::effective_permissions_of(&state.domain, state.net_url.as_deref(), user)
                .await;
        if let Ok(p) = p {
            perms.insert(user.clone(), p);
        }
    }
    let Some(url) = state.net_url.clone() else {
        return fail("the network store is not configured".into());
    };
    let done = tokio::task::spawn_blocking(move || {
        let mut conn = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        mailrs_intake::sieve_policy::survey(&mut conn, &accounts, &domains, |user, from| {
            perms.get(user).is_some_and(|p| p.may_send_as(from))
        })
    })
    .await;
    match done {
        Ok(Ok(s)) => Json(s).into_response(),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(e.to_string()),
    }
}
