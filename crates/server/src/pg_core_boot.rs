//! The `mailrs-pg-core` entry point.
//!
//! Its own file because `lib.rs` reached 572 lines with it inline and the
//! limit is 500 (`rules/common/file-size.md`) — and because it is a second
//! way to start this crate, which is worth being able to find by name rather
//! than by scrolling past the monolith's boot.
//!
//! `pub(crate)` on the module, `pub` on the one function: the binary in
//! `src/bin/pg_core.rs` is a separate crate and can only reach `pub` items,
//! and this is the only one it needs. `core_rpc` stays private.

#![cfg(feature = "core-rpc")]

use crate::{core_rpc, domain_store, health, pg};

/// Boot the SQL core and nothing else.
///
/// The pg-core process serves the core-api contract from the SQL backend and
/// leaves every protocol to the processes that already own it — SMTP to
/// `mailrs-receiver`, the web API to `mailrs-webapi`, outbound to
/// `mailrs-fastcore-sender`. `.claude/rfcs/20260722-monolith-out-of-image.md`
/// set that shape: reviving the fat process was explicitly not the goal, and
/// the switch it enables is `webapi`'s `MAILRS_CORE_RPC_BASE` pointing here
/// instead of at fastcore.
///
/// One entry point rather than `pub mod core_rpc`, so the module stays private
/// and the binary can reach exactly this.
///
/// Env, all of it: `MAILRS_PG_URL` (required — there is no degraded mode worth
/// having for a process whose only job is the SQL backend), `MAILRS_MAILDIR`,
/// `MAILRS_KEVY_URL` for the shared side-state families, and the two
/// `spawn_core_rpc` reads itself, `MAILRS_CORE_RPC_ADDR` and
/// `MAILRS_CORE_API_SECRET`.
pub async fn run_pg_core() {
    use std::sync::Arc;

    let pg_url = std::env::var("MAILRS_PG_URL")
        .expect("MAILRS_PG_URL is required — pg-core has no other backend");
    let maildir_root =
        std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".to_string());
    let net_url = std::env::var("MAILRS_KEVY_URL")
        .ok()
        .filter(|s| !s.is_empty());
    if net_url.is_none() {
        // Not fatal, and worth saying out loud: the fifteen core-sidestate
        // families are mounted from the same source as fastcore's and read the
        // shared network store. Without a URL they answer as though nothing is
        // there — drafts, signatures, webhooks and the audit trail all empty.
        tracing::warn!("MAILRS_KEVY_URL unset — the shared side-state families will read empty");
    }

    // Five minutes, matching the monolith: it covers the WAL-replay boot race
    // that had this lane coming up degraded, with headroom.
    let pool = pg::connect_pool_with_retry(&pg_url, std::time::Duration::from_secs(300))
        .await
        .expect("pg-core could not open its backend");

    let health = health::HealthState::new();
    let state = Arc::new(core_rpc::CoreRpcState {
        mailbox: Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone())),
        domain: Arc::new(domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
            health.clone(),
        )),
        pool: pool.clone(),
        maildir_root,
        net_url,
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let server = core_rpc::spawn_core_rpc(state, shutdown_rx);
    tracing::info!("mailrs-pg-core serving the core-api contract (backend=pg)");

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

    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);

    // Order matters here, and the two halves pull against each other.
    //
    // The pool must be dropped: on the spg backend its `Drop` releases the
    // embedded catalog lock, and dying without it leaves `/data/spg/*.lock`
    // behind — which since spg 7.27 a replacement container reads as a
    // foreign-namespace lock and refuses to open, the exact reason the
    // v1.7.150 deploy came up degraded.
    //
    // And then the process must actually leave. `mailrs-fastcore` flushed its
    // store on SIGTERM, logged that it had, and sat there for want of this —
    // 40 s and counting, on every deploy (see its `exit_after_flush`). Nothing
    // here holds a blocking task the way that did, but relying on that is how
    // the same bug arrives twice.
    // Await the server first. It holds an `Arc<CoreRpcState>`, which holds a
    // pool clone, and the lock releases when the *last* clone drops — so
    // dropping only this one would be a race rather than a release. The task
    // ends on the signal above (`with_graceful_shutdown`), which is why
    // `spawn_core_rpc` hands its handle back.
    let _ = server.await;
    drop(pool);
    tracing::info!("backend released — exiting");
    std::process::exit(0);
}
