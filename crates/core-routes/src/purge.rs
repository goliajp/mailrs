//! Purging deleted accounts and deleted groups: the routes, the purge at
//! the delete when retention is 0 days, and the task that purges what is
//! due. The shared store's part is `core_sidestate::families::account_purge`
//! and `group_purge`; the store's own part is [`PurgeStore`].

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use kevy_client::Connection;
use mailrs_core_api::method::admin::{
    AccountPurgeResponse, AccountPurgeRow, GroupPurgeResponse, PurgeCount,
};
use mailrs_core_sidestate::families::account_purge::{self as purge, maildir, pending};
use mailrs_core_sidestate::families::group_purge;
use purge::registry::Store;

use crate::store::{PurgeStore, StoreError};

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn failed(what: &'static str) -> impl Fn(std::io::Error) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "account purge: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

fn store_failed(what: &'static str) -> impl Fn(StoreError) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "purge: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// `f` on the shared store, on a blocking thread: 503 when it cannot be
/// reached, 500 when `f` fails.
async fn net<S: PurgeStore, T: Send + 'static>(
    state: &Arc<S>,
    what: &'static str,
    f: impl FnOnce(&mut Connection) -> std::io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    let st = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = st.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        f(&mut c).map_err(failed(what))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

/// One pass: every pending address whose `retention_days` have passed (or
/// only `only`), counted, and removed unless `dry_run`. An address that is
/// an account again is skipped.
pub async fn run<S: PurgeStore>(
    state: &Arc<S>,
    dry_run: bool,
    only: Option<String>,
    now: i64,
    retention_days: u64,
) -> Result<AccountPurgeResponse, StatusCode> {
    let due = net(state, "reading what is due", move |c| {
        purge::due(c, now, retention_days, only.as_deref())
    })
    .await?;
    let mut rows = Vec::new();
    let mut skipped_live = Vec::new();
    for (address, deleted_at) in &due.due {
        let live = state
            .exists(address)
            .await
            .map_err(store_failed("reading the account"))?;
        if live {
            skipped_live.push(address.clone());
            continue;
        }
        rows.push(purge_one(state, address, *deleted_at, dry_run, now).await?);
    }
    Ok(AccountPurgeResponse {
        dry_run,
        retention_days,
        pending: due.pending,
        due: due.due.len(),
        rows,
        skipped_live,
    })
}

async fn unregistered<S: PurgeStore>(
    state: &Arc<S>,
    address: &str,
) -> Result<Vec<PurgeCount>, StatusCode> {
    let a = address.to_string();
    let mut out = net(state, "listing unregistered keys", move |c| {
        purge::unregistered(c, &a)
    })
    .await?;
    out.extend(
        state
            .mail_unregistered(address)
            .await
            .map_err(store_failed("listing the store's unregistered records"))?,
    );
    Ok(out)
}

async fn purge_one<S: PurgeStore>(
    state: &Arc<S>,
    address: &str,
    deleted_at: i64,
    dry_run: bool,
    now: i64,
) -> Result<AccountPurgeRow, StatusCode> {
    let dirs = maildir::parked(state.maildir_root(), address)
        .map_err(failed("finding the parked maildir"))?;
    let (maildir_files, maildir_bytes) =
        maildir::measure(&dirs).map_err(failed("measuring the parked maildir"))?;
    let mail_store = state
        .count_mail(address)
        .await
        .map_err(store_failed("counting the store's records"))?;
    let a = address.to_string();
    let network = net(state, "counting the shared store", move |c| {
        purge::count_store(c, Store::Network, &a)
    })
    .await?;
    let mut row = AccountPurgeRow {
        address: address.to_string(),
        deleted_at,
        maildir_files,
        maildir_bytes,
        mail_store,
        network,
        unregistered: Vec::new(),
    };
    if dry_run {
        row.unregistered = unregistered(state, address).await?;
        return Ok(row);
    }
    state
        .remove_mail(address)
        .await
        .map_err(store_failed("removing the store's records"))?;
    let a = address.to_string();
    net(state, "removing the shared store's keys", move |c| {
        purge::remove_store(c, Store::Network, &a)
    })
    .await?;
    maildir::remove(state.maildir_root(), &dirs).map_err(failed("removing the parked maildir"))?;
    row.unregistered = unregistered(state, address).await?;
    let done = row.clone();
    net(state, "recording the purge", move |c| {
        purge::finish(c, &done, now)
    })
    .await?;
    tracing::info!(%address, files = maildir_files, "deleted account purged");
    Ok(row)
}

/// One group pass: the shared store's team state of every due group (or
/// only `only`), and the reply claims of its conversations in the store.
pub async fn run_groups<S: PurgeStore>(
    state: &Arc<S>,
    dry_run: bool,
    only: Option<i64>,
    now: i64,
    retention_days: u64,
) -> Result<GroupPurgeResponse, StatusCode> {
    let pass = net(state, "reading the due groups", move |c| {
        group_purge::due(c, now, retention_days, only)
    })
    .await?;
    let mut rows = Vec::new();
    for g in &pass.due {
        let mail = vec![
            state
                .purge_group_replies(g.gid, dry_run)
                .await
                .map_err(store_failed("purging the group's reply claims"))?,
        ];
        let g = g.clone();
        let row = net(state, "purging the group", move |c| {
            let row = group_purge::row(&g, group_purge::network(c, &g, dry_run)?, mail);
            if !dry_run {
                group_purge::finish(c, &row, now)?;
            }
            Ok(row)
        })
        .await?;
        rows.push(row);
    }
    Ok(pass.response(dry_run, retention_days, rows))
}

fn yes() -> bool {
    true
}

#[derive(Debug, serde::Deserialize)]
pub struct PurgeQuery {
    #[serde(default = "yes")]
    dry_run: bool,
    address: Option<String>,
}

/// POST /v1/admin/maintenance:account-purge
pub async fn account_purge<S: PurgeStore>(
    State(state): State<Arc<S>>,
    Query(q): Query<PurgeQuery>,
) -> Result<Json<AccountPurgeResponse>, StatusCode> {
    let days = pending::retention_days();
    run(&state, q.dry_run, q.address, now(), days)
        .await
        .map(Json)
}

#[derive(Debug, serde::Deserialize)]
pub struct GroupPurgeQuery {
    #[serde(default = "yes")]
    dry_run: bool,
    gid: Option<i64>,
}

/// POST /v1/admin/maintenance:group-purge
pub async fn group_purge<S: PurgeStore>(
    State(state): State<Arc<S>>,
    Query(q): Query<GroupPurgeQuery>,
) -> Result<Json<GroupPurgeResponse>, StatusCode> {
    let days = group_purge::retention_days();
    run_groups(&state, q.dry_run, q.gid, now(), days)
        .await
        .map(Json)
}

/// Purge `address` now, for a delete under a retention of 0 days. Failures
/// are logged: the retention task retries.
pub async fn purge_now<S: PurgeStore>(state: Arc<S>, address: String) {
    if let Err(code) = run(&state, false, Some(address.clone()), now(), 0).await {
        tracing::error!(%code, %address, "purging at the delete failed; the retention task retries");
    }
}

const BASE: Duration = Duration::from_secs(3600);
const MAX: Duration = Duration::from_secs(24 * 3600);

/// The wait after a pass that purged `purged` accounts and groups:
/// doubling while nothing is due, back to hourly after one that purged.
fn next_wait(wait: Duration, purged: usize) -> Duration {
    match purged {
        0 => (wait * 2).min(MAX),
        _ => BASE,
    }
}

/// Purge what is due, deleted accounts and deleted groups' team state, for
/// ever. A pass with nothing due reads two hashes and writes nothing.
pub fn spawn<S: PurgeStore>(state: Arc<S>) {
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let days = pending::retention_days();
            let accounts = run(&state, false, None, now(), days)
                .await
                .map_or(0, |r| r.rows.len());
            let groups = run_groups(&state, false, None, now(), days)
                .await
                .map_or(0, |r| r.rows.len());
            wait = next_wait(wait, accounts + groups);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_purge_backs_off_to_a_day_and_work_resets_it() {
        let mut w = BASE;
        for _ in 0..10 {
            w = next_wait(w, 0);
        }
        assert_eq!(w, MAX);
        assert_eq!(next_wait(w, 2), BASE);
    }
}
