//! Local PG-backed greylist white/black lists — the spg-bound loader half
//! (S5.3).
//!
//! The pure snapshot type, matching logic, and admin-input
//! normalization/validation moved to `mailrs_receiver::greylist_local`
//! (free of spg). This module keeps the PG loaders (`reload` /
//! `load_from_pg` / `spawn_reload_task`) that bind the spg `BackendPool`,
//! and re-exports the receiver half so every `crate::greylist_local::*`
//! call site stays unchanged.

use std::str::FromStr;
use std::time::Duration;

use ipnet::IpNet;

pub use mailrs_receiver::greylist_local::*;

/// Query PG for all rows, build a fresh snapshot, and atomically install
/// it. Errors are recorded on the handle (`last_error`) but never panic.
pub async fn reload(handle: &GreylistLocalHandle, pool: &crate::pg::BackendPool) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match load_from_pg(pool).await {
        Ok(mut snapshot) => {
            snapshot.last_reload_at = Some(now);
            snapshot.last_error = None;
            let total = snapshot.total();
            let white = snapshot.white_count();
            let black = snapshot.black_count();
            *handle.write().await = snapshot;
            tracing::debug!(
                target: "greylist.local",
                total,
                white,
                black,
                "greylist_local snapshot reloaded"
            );
            metrics::counter!("mailrs_greylist_local_reload_total", "outcome" => "ok").increment(1);
            metrics::gauge!("mailrs_greylist_local_size", "list" => "white", "kind" => "any")
                .set(white as f64);
            metrics::gauge!("mailrs_greylist_local_size", "list" => "black", "kind" => "any")
                .set(black as f64);
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::warn!(
                target: "greylist.local",
                error = %msg,
                "greylist_local reload failed; keeping previous snapshot"
            );
            let mut g = handle.write().await;
            g.last_error = Some(msg);
            metrics::counter!("mailrs_greylist_local_reload_total", "outcome" => "error")
                .increment(1);
        }
    }
}

async fn load_from_pg(pool: &crate::pg::BackendPool) -> Result<GreylistLocalLists, sqlx::Error> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT kind, list, value FROM greylist_local_lists")
            .fetch_all(pool)
            .await?;

    let mut s = GreylistLocalLists::default();
    for (kind, list, value) in rows {
        let is_black = list == "black";
        match kind.as_str() {
            "domain" => {
                let v = value.to_lowercase();
                if is_black {
                    s.black_domains.insert(v);
                } else {
                    s.white_domains.insert(v);
                }
            }
            "email" => {
                let v = value.to_lowercase();
                if is_black {
                    s.black_emails.insert(v);
                } else {
                    s.white_emails.insert(v);
                }
            }
            "cidr" => {
                if let Ok(net) = IpNet::from_str(&value) {
                    if is_black {
                        s.black_cidrs.push(net);
                    } else {
                        s.white_cidrs.push(net);
                    }
                } else {
                    tracing::warn!(
                        target: "greylist.local",
                        value = %value,
                        "skipping unparseable cidr row"
                    );
                }
            }
            other => {
                tracing::warn!(
                    target: "greylist.local",
                    kind = %other,
                    "skipping row with unknown kind"
                );
            }
        }
    }
    Ok(s)
}

/// Spawn a background task that periodically refreshes the snapshot.
///
/// Cadence is `interval_secs`. First reload runs immediately so the
/// snapshot is populated before any inbound mail is accepted (the boot
/// path also calls `reload` synchronously for the same reason — this task
/// is the periodic refresher, not the boot loader).
pub fn spawn_reload_task(
    handle: GreylistLocalHandle,
    pool: crate::pg::BackendPool,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        // first tick fires instantly — we already reloaded at boot, skip it
        tick.tick().await;
        loop {
            tick.tick().await;
            reload(&handle, &pool).await;
        }
    })
}
