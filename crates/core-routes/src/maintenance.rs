//! Operator passes over the shared store that both cores run alike: quota
//! limits, usage drift, the sent-to lists and the Sieve survey. The logic
//! is in `core_sidestate` and `intake`; these routes give it the store's
//! accounts and its connection to the shared store.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use mailrs_core_api::method::admin::{
    QuotaLimitsResponse, SentToBackfillResponse, UsageDriftResponse,
};
use mailrs_core_sidestate::families::{contact_sent, quota_limits, usage_drift};
use mailrs_intake::sieve_policy::Survey;

use crate::store::{AccountStore, MaintenanceStore, StoreError};

#[derive(serde::Deserialize)]
pub struct DryRunQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

fn dry_run_unless_asked() -> bool {
    true
}

fn store_failed(what: &str) -> impl Fn(StoreError) -> StatusCode + '_ {
    move |e| {
        tracing::error!(error = %e, "{what}: reading the store failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Run `f` against the shared store on a blocking thread. A store with no
/// shared store configured, or one that cannot be reached, answers 503:
/// there is nothing these passes could read or write. A failing pass is
/// 500.
async fn on_net<S: AccountStore, T: Send + 'static>(
    state: &Arc<S>,
    what: &'static str,
    f: impl FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    if !state.net_configured() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let st = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = st.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        f(&mut c).map_err(|e| {
            tracing::error!(error = %e, "{what} failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "{what} panicked");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
}

/// POST /v1/admin/maintenance:quota-limits — every account's usage against
/// the default limit; `?dry_run=false` gives the default to the accounts
/// with no limit that are not near full.
pub async fn quota_limits<S: MaintenanceStore>(
    State(state): State<Arc<S>>,
    Query(q): Query<DryRunQuery>,
) -> Result<Json<QuotaLimitsResponse>, StatusCode> {
    let users = state
        .account_addresses()
        .await
        .map_err(store_failed("quota limits"))?;
    on_net(&state, "quota limits", move |c| {
        quota_limits::fill_in(c, &users, q.dry_run)
    })
    .await
    .map(Json)
}

async fn usage_drift_pass<S: MaintenanceStore>(
    state: Arc<S>,
    dry_run: bool,
) -> Result<Json<UsageDriftResponse>, StatusCode> {
    let users = state
        .account_addresses()
        .await
        .map_err(store_failed("usage drift"))?;
    let root = state.maildir_root().to_path_buf();
    let now = chrono::Utc::now().timestamp();
    on_net(&state, "usage drift", move |c| {
        usage_drift::pass(c, &root, &users, dry_run, now)
    })
    .await
    .map(Json)
}

/// GET /v1/admin/maintenance:usage-drift — how far each account's usage
/// counter is from what its maildir takes on disk; reports only.
pub async fn usage_drift<S: MaintenanceStore>(
    State(state): State<Arc<S>>,
) -> Result<Json<UsageDriftResponse>, StatusCode> {
    usage_drift_pass(state, true).await
}

/// POST /v1/admin/maintenance:usage-drift — reports, and with
/// `?dry_run=false` sets each counter to its disk figure and deletes the
/// counters of addresses that are not accounts.
pub async fn usage_drift_apply<S: MaintenanceStore>(
    State(state): State<Arc<S>>,
    Query(q): Query<DryRunQuery>,
) -> Result<Json<UsageDriftResponse>, StatusCode> {
    usage_drift_pass(state, q.dry_run).await
}

/// POST /v1/admin/maintenance:backfill-sent-to — for each user, the
/// addresses they have sent mail to, from the relationship counters. Adds
/// only, so a second run reports nothing missing.
pub async fn backfill_sent_to<S: MaintenanceStore>(
    State(state): State<Arc<S>>,
    Query(q): Query<DryRunQuery>,
) -> Result<Json<SentToBackfillResponse>, StatusCode> {
    let dry = q.dry_run;
    on_net(&state, "backfill-sent-to", move |c| {
        contact_sent::rebuild_sent_to(c, dry)
    })
    .await
    .map(Json)
}

/// GET /v1/admin/maintenance:sieve-survey — which stored Sieve scripts the
/// forwarding rules change: counts, accounts and domains, never script
/// text. Each account's permissions are read before the scripts, so the
/// survey's send-as test is the one sending applies.
pub async fn sieve_survey<S: MaintenanceStore>(
    State(state): State<Arc<S>>,
) -> Result<Json<Survey>, StatusCode> {
    let accounts = state
        .account_addresses()
        .await
        .map_err(store_failed("sieve survey"))?;
    let names = state
        .domain_names()
        .await
        .map_err(store_failed("sieve survey"))?;
    let domains = mailrs_intake::domains::LocalDomainsCache::default()
        .get(move || Ok(names))
        .to_vec();
    let mut perms = HashMap::new();
    for user in &accounts {
        if let Some(p) = state.send_as_permissions(user).await {
            perms.insert(user.clone(), p);
        }
    }
    on_net(&state, "sieve survey", move |c| {
        mailrs_intake::sieve_policy::survey(c, &accounts, &domains, |user, from| {
            perms.get(user).is_some_and(|p| p.may_send_as(from))
        })
    })
    .await
    .map(Json)
}
