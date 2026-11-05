//! Purging deleted accounts on fastcore: the route, the retention task,
//! and the embedded store's part. The registry and the network part are
//! `mailrs_core_sidestate::families::account_purge`, shared with the SQL
//! core.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use kevy_client::Connection;
use mailrs_core_api::method::admin::{AccountPurgeResponse, AccountPurgeRow, PurgeCount};
use mailrs_core_sidestate::families::account_purge::{self as purge, maildir, pending};
use purge::registry::{Store, spellings};

use crate::FastcoreState;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The network kevy for the delete's mark: `None` only when none is
/// configured (tests); one that is configured and unreachable is a 503,
/// since a delete that cannot mark would never be purged.
pub(crate) fn mark_conn(state: &FastcoreState) -> Result<Option<Connection>, StatusCode> {
    if state.net_url.is_none() {
        return Ok(None);
    }
    match state.net_conn() {
        Some(c) => Ok(Some(c)),
        None => {
            tracing::error!("account purge: the network kevy is unreachable");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// One pass: every pending address whose `retention_days` have passed (or
/// only `only`), counted, and removed unless `dry_run`.
pub(crate) fn run(
    state: &FastcoreState,
    net: &mut Connection,
    dry_run: bool,
    only: Option<&str>,
    now: i64,
    retention_days: u64,
) -> io::Result<AccountPurgeResponse> {
    let due = purge::due(net, now, retention_days, only)?;
    let mut rows = Vec::new();
    let mut skipped_live = Vec::new();
    for (address, deleted_at) in &due.due {
        if state.mailbox.get_account_blob(address)?.is_some() {
            skipped_live.push(address.clone());
            continue;
        }
        rows.push(purge_one(state, net, address, *deleted_at, dry_run, now)?);
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

fn mail_conn(state: &FastcoreState) -> Connection {
    Connection::Embedded(Box::new(state.mailbox.store_ref().clone()))
}

fn mail_unregistered(mail: &mut Connection, address: &str) -> io::Result<Vec<PurgeCount>> {
    let mut out = purge::unregistered(mail, address)?;
    for p in &mut out {
        p.name = format!("mail:{}", p.name);
    }
    Ok(out)
}

fn purge_one(
    state: &FastcoreState,
    net: &mut Connection,
    address: &str,
    deleted_at: i64,
    dry_run: bool,
    now: i64,
) -> io::Result<AccountPurgeRow> {
    let dirs = maildir::parked(Path::new(&state.maildir_root), address)?;
    let (maildir_files, maildir_bytes) = maildir::measure(&dirs)?;
    let mut mail = mail_conn(state);
    let mail_store = purge::count_store(&mut mail, Store::Mail, address)?;
    let network = purge::count_store(net, Store::Network, address)?;
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
        let mut u = purge::unregistered(net, address)?;
        u.extend(mail_unregistered(&mut mail, address)?);
        row.unregistered = u;
        return Ok(row);
    }
    // conversation by conversation first: a conversation's shared records
    // stay while another owner holds it, and go with the last
    for spelling in spellings(address) {
        for tid in state.mailbox.all_thread_ids_for_user(&spelling)? {
            state.mailbox.delete_thread(&spelling, &tid)?;
        }
    }
    purge::remove_store(&mut mail, Store::Mail, address)?;
    purge::remove_store(net, Store::Network, address)?;
    maildir::remove(&dirs)?;
    let mut u = purge::unregistered(net, address)?;
    u.extend(mail_unregistered(&mut mail, address)?);
    row.unregistered = u;
    purge::finish(net, &row, now)?;
    tracing::info!(%address, files = maildir_files, "deleted account purged");
    Ok(row)
}

fn yes() -> bool {
    true
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct PurgeQuery {
    #[serde(default = "yes")]
    dry_run: bool,
    address: Option<String>,
}

/// `POST /v1/admin/maintenance:account-purge`.
pub(crate) async fn account_purge_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<PurgeQuery>,
) -> Result<Json<AccountPurgeResponse>, StatusCode> {
    let done = tokio::task::spawn_blocking(move || {
        let mut net = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let days = pending::retention_days();
        run(
            &state,
            &mut net,
            q.dry_run,
            q.address.as_deref(),
            now(),
            days,
        )
        .map_err(|e| {
            tracing::error!(error = %e, "account purge failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    done.map(Json)
}

/// Purge `address` now, for a delete under a retention of 0 days.
pub(crate) async fn purge_now(state: Arc<FastcoreState>, address: String) {
    let done = tokio::task::spawn_blocking(move || {
        let mut net = state
            .net_conn()
            .ok_or_else(|| io::Error::other("no network kevy"))?;
        run(&state, &mut net, false, Some(&address), now(), 0)
    })
    .await;
    match done {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            tracing::error!(error = %e, "purging at the delete failed; the retention task retries")
        }
        Err(e) => tracing::error!(error = %e, "purging at the delete panicked"),
    }
}

const BASE: Duration = Duration::from_secs(3600);
const MAX: Duration = Duration::from_secs(24 * 3600);

/// The wait after a pass that purged `purged` accounts: doubling while
/// there is nothing due, back to hourly after one that purged.
pub(crate) fn next_wait(wait: Duration, purged: usize) -> Duration {
    match purged {
        0 => (wait * 2).min(MAX),
        _ => BASE,
    }
}

/// Purge what is due, for ever. A pass with nothing due reads one hash and
/// writes nothing.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let st = state.clone();
            let purged = tokio::task::spawn_blocking(move || {
                let mut net = st.net_conn()?;
                let days = pending::retention_days();
                run(&st, &mut net, false, None, now(), days)
                    .map_err(|e| tracing::warn!(error = %e, "account purge pass failed"))
                    .ok()
            })
            .await
            .ok()
            .flatten()
            .map_or(0, |r| r.rows.len());
            wait = next_wait(wait, purged);
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
        assert_eq!(next_wait(w, 1), BASE);
    }
}
