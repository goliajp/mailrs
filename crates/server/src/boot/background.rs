//! The background work a boot starts beside the listeners.

use std::sync::Arc;

use crate::*;

use super::Roles;

pub(super) fn spawn_ics_feed_worker(pg_pool: &Option<pg::BackendPool>) {
    // MRS-10: spawn external ICS feed worker. Cheap when no feeds exist —
    // the DUE query returns empty and the loop sleeps.
    if let Some(pool) = pg_pool {
        calendar::feed_worker::spawn_feed_worker(pool.clone());
        tracing::info!(event = "subsystem_started", subsystem = "external_ics_feed");
    }
}

pub(super) fn start_greylist_whitelist_sync(
    cfg: &ServerConfig,
) -> greylist_sync::GreylistListsHandle {
    // Greylist whitelist sync: fires once immediately, then every
    // cfg.greylist_sync_interval_secs. Empty URL = disabled, handle stays
    // empty (no hits, all senders go through the triplet check). The
    // handle is cloned into the GreylistStage so the sync task and the
    // stage share a single snapshot via tokio::sync::RwLock.
    let greylist_whitelist = greylist_sync::empty();
    if let Some(ref url) = cfg.greylist_whitelist_url {
        let handle = greylist_whitelist.clone();
        let url = url.clone();
        let interval = cfg.greylist_sync_interval_secs;
        tracing::info!(
            event = "subsystem_started",
            subsystem = "greylist_sync",
            url = %url,
            interval_secs = interval,
        );
        // monolith: no disk cache — its whitelist
        // behaviour predates the receiver-split hardening and stays as-was
        greylist_sync::spawn_sync_task(handle, url, interval, None);
    } else {
        tracing::info!(
            event = "subsystem_skipped",
            subsystem = "greylist_sync",
            reason = "MAILRS_GREYLIST_WHITELIST_URL not set"
        );
    }
    greylist_whitelist
}

/// The spool the SMTP side writes to and the drain reads.
pub(super) fn spool_root(cfg: &ServerConfig) -> String {
    std::env::var("MAILRS_SPOOL_ROOT").unwrap_or_else(|_| format!("{}/.spool", cfg.maildir_root))
}

pub(super) fn start_spool_drain(
    roles: Roles,
    ctx: &Arc<ConnectionContext>,
    cfg: &ServerConfig,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    mailbox_store: &Option<Arc<PgMailboxStore>>,
) {
    // When this core runs as the consumer half (MAILRS_RECEIVER_SPLIT, or
    // the peer role), drain the spool the receiver process writes to.
    if roles.spool_drain
        || std::env::var("MAILRS_RECEIVER_SPLIT")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(false)
    {
        spawn_spool_drain(ctx, cfg, domain_store, mailbox_store);
    }
}

fn spawn_spool_drain(
    ctx: &Arc<ConnectionContext>,
    cfg: &ServerConfig,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    mailbox_store: &Option<Arc<PgMailboxStore>>,
) {
    let (Some(domain), Some(mailbox)) = (domain_store.clone(), mailbox_store.clone()) else {
        tracing::error!("no database: the spool cannot be routed and is not drained");
        return;
    };
    let spool_root = spool_root(cfg);
    let net_url = std::env::var("MAILRS_KEVY_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let core = Arc::new(crate::intake::PgCore {
        dir: crate::intake::PgDirectory::new(domain, cfg.maildir_root.clone(), net_url),
        mailbox,
        event_bus: ctx.event_bus.clone(),
    });
    // the drain queues webhooks for arriving mail on the network kevy; this
    // core sends them, as the kevy core does
    if let Some(url) = core.dir.net_url.clone() {
        tokio::spawn(mailrs_intake::webhook_delivery::run(Arc::new(move || {
            kevy_client::Connection::connect(&url).ok()
        })));
    }
    crate::intake::spawn_drain(
        core,
        std::path::Path::new(&spool_root),
        std::time::Duration::from_secs(15),
    );
}

pub(super) fn spawn_snooze_wake(mailbox_store: &Option<Arc<PgMailboxStore>>) {
    // Bring back the threads whose time has come.
    //
    // A snooze files the thread away — `archived` with the time beside it, the
    // same shape the kevy lane uses — so something has to un-archive it. A
    // predicate on the read cannot: the row is archived, and "archived because
    // asleep, and no longer asleep" is not a state the list can tell from
    // "archived on purpose".
    //
    // A minute is the resolution, matching the kevy side. A thread asked back
    // "tomorrow morning" arriving at 08:00:37 is the same promise kept, and a
    // second-accurate wake would cost sixty times the ticks to say nothing sixty
    // times as often.
    //
    // Logged only when something happened. A line every minute saying zero is
    // the shape that turned the maildir sweep's own idle report into the noise
    // hiding it.
    if let Some(mb) = mailbox_store.clone() {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await; // the first tick is immediate; skip it
            loop {
                tick.tick().await;
                match mb.wake_snoozed().await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(woken = n, "snoozed threads returned"),
                    Err(e) => tracing::warn!(error = %e, "snooze wake failed"),
                }
            }
        });
        tracing::info!(event = "subsystem_started", subsystem = "snooze_wake");
    }
}

// Phase 2 — optional core RPC server (only compiled with --features core-rpc).
// Default build excludes this entirely; production artifact is byte-identical.
#[cfg(feature = "core-rpc")]
pub(super) fn spawn_core_rpc_server(
    cfg: &ServerConfig,
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    pg_pool: &Option<pg::BackendPool>,
    shutdown_rx: &tokio::sync::watch::Receiver<bool>,
) {
    if let (Some(mb), Some(ds), Some(pool)) = (
        mailbox_store.as_ref(),
        domain_store.as_ref(),
        pg_pool.as_ref(),
    ) {
        let core_rpc_state = std::sync::Arc::new(core_rpc::CoreRpcState {
            mailbox: mb.clone(),
            domain: ds.clone(),
            pool: pool.clone(),
            maildir_root: cfg.maildir_root.clone(),
            net_url: std::env::var("MAILRS_KEVY_URL")
                .ok()
                .filter(|s| !s.is_empty()),
        });
        // Detached on purpose. The monolith has no lock to release that this
        // task holds — its own shutdown path covers that — so the handle is
        // dropped rather than awaited. `drop` and not `let _ =`: a JoinHandle
        // is a future, and `let _ =` on one reads as "ignore the result" while
        // actually detaching it (clippy::let_underscore_future).
        drop(core_rpc::spawn_core_rpc(
            core_rpc_state,
            shutdown_rx.clone(),
        ));
    }
}

pub(super) async fn wait_for_shutdown_signal() {
    // keep main alive — exit on SIGINT (interactive ctrl+c) or
    // SIGTERM (docker stop / compose recreate). SIGTERM matters for
    // the embedded spg catalog lock: dying without running Drop
    // leaves /data/spg/*.lock behind, and since spg 7.27 a
    // replacement container sees a foreign-namespace lock as
    // undecidable and refuses to open the catalog (v1.7.150 deploy
    // came up degraded for exactly this reason).
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.expect("failed to listen for ctrl+c"),
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
}
