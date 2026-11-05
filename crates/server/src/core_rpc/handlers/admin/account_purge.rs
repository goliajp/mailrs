//! Purging deleted accounts on the SQL core: the same pass as fastcore's,
//! with this core's rows taken by table. The registry, the maildir and the
//! network part are `mailrs_core_sidestate::families::account_purge`. The
//! same task and a route of their own purge deleted email groups' team
//! state (`group_purge`) and their reply claims in this core's table, as
//! fastcore does in its store.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use kevy_client::Connection;
use mailrs_core_api::method::admin::{
    AccountPurgeResponse, AccountPurgeRow, GroupPurgeResponse, PurgeCount,
};
use mailrs_core_sidestate::NetKevy;
use mailrs_core_sidestate::families::account_purge::{self as purge, maildir, pending};
use mailrs_core_sidestate::families::group_purge;
use purge::registry::{SQL_CASCADED, SQL_TABLES, Store};

use crate::core_rpc::CoreRpcState;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn failed(what: &'static str) -> impl Fn(io::Error) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "account purge: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

fn sql_failed(e: sqlx::Error) -> StatusCode {
    tracing::error!(error = %e, "account purge: the database failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Run `f` on the network kevy, off the async workers.
async fn net<T: Send + 'static>(
    state: &Arc<CoreRpcState>,
    f: impl FnOnce(&mut Connection) -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    let st = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = st.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        f(&mut c).map_err(failed("the network kevy"))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub(crate) async fn is_account(state: &CoreRpcState, address: &str) -> Result<bool, StatusCode> {
    let found: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM accounts WHERE address = $1 AND deleted_at IS NULL")
            .bind(address)
            .fetch_optional(&state.pool)
            .await
            .map_err(sql_failed)?;
    Ok(found.is_some())
}

/// The account's rows, by table, and its messages, which go with its
/// mailboxes.
async fn count_rows(state: &CoreRpcState, address: &str) -> Result<Vec<PurgeCount>, StatusCode> {
    let mut out = Vec::new();
    let messages = "SELECT COUNT(*) FROM messages WHERE mailbox_id IN \
                    (SELECT id FROM mailboxes WHERE user_address = $1)";
    let (n,): (i64,) = sqlx::query_as(messages)
        .bind(address)
        .fetch_one(&state.pool)
        .await
        .map_err(sql_failed)?;
    if n > 0 {
        out.push(PurgeCount {
            name: "messages".into(),
            count: n as u64,
        });
    }
    let rows = SQL_TABLES
        .iter()
        .chain(SQL_CASCADED)
        .chain(&[("accounts", "address")]);
    for (table, column) in rows {
        let q = format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1");
        let (n,): (i64,) = sqlx::query_as(&q)
            .bind(address)
            .fetch_one(&state.pool)
            .await
            .map_err(sql_failed)?;
        if n > 0 {
            out.push(PurgeCount {
                name: table.to_string(),
                count: n as u64,
            });
        }
    }
    Ok(out)
}

async fn delete_rows(state: &CoreRpcState, address: &str) -> Result<(), StatusCode> {
    let mut tx = state.pool.begin().await.map_err(sql_failed)?;
    for (table, column) in SQL_TABLES {
        let q = format!("DELETE FROM {table} WHERE {column} = $1");
        sqlx::query(&q)
            .bind(address)
            .execute(&mut *tx)
            .await
            .map_err(sql_failed)?;
    }
    // the row last: what references it goes with it
    sqlx::query("DELETE FROM accounts WHERE address = $1 AND deleted_at IS NOT NULL")
        .bind(address)
        .execute(&mut *tx)
        .await
        .map_err(sql_failed)?;
    tx.commit().await.map_err(sql_failed)
}

/// One pass: every pending address whose `retention_days` have passed (or
/// only `only`), counted, and removed unless `dry_run`.
pub(crate) async fn run(
    state: &Arc<CoreRpcState>,
    dry_run: bool,
    only: Option<String>,
    now: i64,
    retention_days: u64,
) -> Result<AccountPurgeResponse, StatusCode> {
    let due = net(state, move |c| {
        purge::due(c, now, retention_days, only.as_deref())
    })
    .await?;
    let mut rows = Vec::new();
    let mut skipped_live = Vec::new();
    for (address, deleted_at) in &due.due {
        if is_account(state, address).await? {
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

async fn purge_one(
    state: &Arc<CoreRpcState>,
    address: &str,
    deleted_at: i64,
    dry_run: bool,
    now: i64,
) -> Result<AccountPurgeRow, StatusCode> {
    let dirs = maildir::parked(Path::new(&state.maildir_root), address)
        .map_err(failed("finding the parked maildir"))?;
    let (maildir_files, maildir_bytes) =
        maildir::measure(&dirs).map_err(failed("measuring the parked maildir"))?;
    let mail_store = count_rows(state, address).await?;
    let a = address.to_string();
    let network = net(state, move |c| purge::count_store(c, Store::Network, &a)).await?;
    let mut row = AccountPurgeRow {
        address: address.to_string(),
        deleted_at,
        maildir_files,
        maildir_bytes,
        mail_store,
        network,
        unregistered: Vec::new(),
    };
    let a = address.to_string();
    if dry_run {
        row.unregistered = net(state, move |c| purge::unregistered(c, &a)).await?;
        return Ok(row);
    }
    delete_rows(state, address).await?;
    let unregistered = net(state, move |c| {
        purge::remove_store(c, Store::Network, &a)?;
        purge::unregistered(c, &a)
    })
    .await?;
    maildir::remove(&dirs).map_err(failed("removing the parked maildir"))?;
    row.unregistered = unregistered;
    let done = row.clone();
    net(state, move |c| purge::finish(c, &done, now)).await?;
    tracing::info!(%address, files = maildir_files, "deleted account purged");
    Ok(row)
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
pub async fn account_purge_route(
    State(state): State<Arc<CoreRpcState>>,
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

/// The table a group purge counts this core's reply claims under.
const REPLY_TABLE: &str = "shared_reply_state";

/// One group pass: the network kevy's part, and the reply claims of the
/// groups' conversations in this core's table, as fastcore's.
pub(crate) async fn run_groups(
    state: &Arc<CoreRpcState>,
    dry_run: bool,
    only: Option<i64>,
    now: i64,
    retention_days: u64,
) -> Result<GroupPurgeResponse, StatusCode> {
    let pass = net(state, move |c| {
        group_purge::due(c, now, retention_days, only)
    })
    .await?;
    let mut rows = Vec::new();
    for g in &pass.due {
        let n = state
            .mailbox
            .purge_group_replies(g.gid, dry_run)
            .await
            .map_err(sql_failed)?;
        let mail = vec![PurgeCount {
            name: REPLY_TABLE.to_string(),
            count: n,
        }];
        let g = g.clone();
        let row = net(state, move |c| {
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

/// POST /v1/admin/maintenance:group-purge
pub async fn group_purge_route(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<GroupPurgeQuery>,
) -> Result<Json<GroupPurgeResponse>, StatusCode> {
    let days = group_purge::retention_days();
    run_groups(&state, q.dry_run, q.gid, now(), days)
        .await
        .map(Json)
}

/// Mark `address` deleted at `now`, for its purge. `Ok(false)` when no
/// network kevy is configured (tests); one configured and unreachable is
/// a 503, since a delete that cannot mark would never be purged.
pub(crate) async fn mark(
    state: &Arc<CoreRpcState>,
    address: &str,
    now: i64,
) -> Result<bool, StatusCode> {
    if state.net_url.is_none() {
        return Ok(false);
    }
    let a = address.to_string();
    net(state, move |c| pending::mark(c, &a, now)).await?;
    Ok(true)
}

/// Undo [`mark`] for a delete that did not happen.
pub(crate) async fn unmark(state: &Arc<CoreRpcState>, address: &str) -> Result<(), StatusCode> {
    let a = address.to_string();
    net(state, move |c| pending::clear(c, &a)).await
}

/// Whether `address` waits for its purge; `false` without a network kevy.
pub(crate) async fn is_pending(
    state: &Arc<CoreRpcState>,
    address: &str,
) -> Result<bool, StatusCode> {
    if state.net_url.is_none() {
        return Ok(false);
    }
    let a = address.to_string();
    net(state, move |c| pending::deleted_at(c, &a))
        .await
        .map(|d| d.is_some())
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
pub(crate) fn spawn(state: Arc<CoreRpcState>) {
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
            let purged = accounts + groups;
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
        assert_eq!(next_wait(w, 2), BASE);
    }
}
