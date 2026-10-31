//! The mailrs-core-api HTTP server over the SQL stores.
//!
//! Wraps `Arc<PgMailboxStore>`, `Arc<DomainStore>` and the rest of this
//! process's state into a `Handler` impl from `mailrs_core_api::server`,
//! mounts the per-method routes, binds `MAILRS_CORE_RPC_ADDR` (default
//! `0.0.0.0:3300`) and checks `Authorization: Bearer <MAILRS_CORE_API_SECRET>`
//! on authenticated routes.

mod handlers;
mod operator_clear;
mod router;
mod router_views;
// Not re-exported wider than it is: `build_full_router` is `pub(super)` in
// `router.rs`, which already reaches this module and the `tests` submodules
// that call it. A `pub(crate) use` here widens past that and is E0364.
use router::build_full_router;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use mailrs_core_api::server::Handler;
use mailrs_core_api::types::{BackendKind, HealthResponse};

/// Aggregate of all state the core RPC server needs to answer requests.
///
/// Cement passes one of these in to `spawn_core_rpc`. The shape mirrors
/// the future `mailrs-core` binary's state struct.
///
pub struct CoreRpcState {
    pub mailbox: Arc<mailrs_mailbox::PgMailboxStore>,
    pub domain: Arc<crate::domain_store::DomainStore>,
    /// Direct pool handle — used by `api_key_store` free functions and
    /// other admin paths that don't go through DomainStore.
    pub pool: crate::pg::BackendPool,
    /// Maildir root — handlers that serve raw bytes look up the file
    /// as `crate::body` lays them out.
    pub maildir_root: String,
    /// Network-kevy URL (`MAILRS_KEVY_URL`) for the SHARED side-state
    /// families (drafts/signatures/templates/webhooks/audit/
    /// contacts/analysis/outbound/groups/api-keys/sieve). The pg-core
    /// serves these from the same network kevy fastcore + webapi use, so
    /// the two cores' side-state routes are byte-identical. `None` when
    /// unset (side-state routes then serve empty).
    pub net_url: Option<String>,
    /// The in-process bus IMAP IDLE listens on; a sent copy filed through
    /// the contract is announced on it, as an arriving message is.
    pub event_bus: crate::event_bus::EventBus,
}

/// Start the Trash purge over this core's store.
pub(crate) fn spawn_trash_purge(state: Arc<CoreRpcState>) {
    handlers::trash::spawn_purge(state);
}

impl mailrs_core_sidestate::NetKevy for CoreRpcState {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        let url = self.net_url.as_ref()?;
        kevy_client::Connection::connect(url).ok()
    }
}

impl Handler for CoreRpcState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            // the SQL core; `fastcore` sets this to `Kevy`
            backend: BackendKind::Pg,
            // healthz is a liveness probe — process is up.
            ready: true,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // Phase 2.6 deep probe: acquire a backend pool connection with a
        // tight 1s timeout + run `SELECT 1`. Failure (timeout / acquire
        // error) flips `ready` to false so the orchestrator routes traffic
        // away. This matches the semantics expected by /api/readiness in
        // the existing webapi.
        let ready = match tokio::time::timeout(
            std::time::Duration::from_millis(1000),
            sqlx::query("SELECT 1").execute(&self.pool),
        )
        .await
        {
            Ok(Ok(_)) => true,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "readyz: backend ping returned error");
                false
            }
            Err(_) => {
                tracing::debug!("readyz: backend ping timed out (>1s)");
                false
            }
        };
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Pg,
            ready,
        }
    }
}

/// Spawn the `mailrs-core-api` server on the bind address from env.
///
/// `MAILRS_CORE_RPC_ADDR` (default `0.0.0.0:3300`) — listen address.
/// `MAILRS_CORE_API_SECRET` — shared bearer secret for authenticated
/// routes. Required: the process exits when it is unset or blank.
///
/// Returns immediately — the server runs in a background tokio task, and its
/// `JoinHandle` comes back so a caller that has to release
/// something after the last request can wait for the task to end.
pub fn spawn_core_rpc(
    state: Arc<CoreRpcState>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let addr = std::env::var("MAILRS_CORE_RPC_ADDR")
        .unwrap_or_else(|_| format!("0.0.0.0:{}", mailrs_core_api::DEFAULT_CORE_RPC_PORT));

    let secret = match mailrs_core_api::required_auth_secret() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(2);
        }
    };

    let router = build_full_router(state, secret);

    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, addr = %addr, "core RPC bind failed");
                return;
            }
        };
        tracing::info!(
            event = "subsystem_started",
            subsystem = "core_rpc",
            addr = %addr,
            "mailrs-core-api server listening"
        );
        let mut shutdown_rx = shutdown_rx;
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            // Drop on `true` — same pattern as the rest of the server.
            let _ = shutdown_rx.changed().await;
        });
        if let Err(e) = server.await {
            tracing::error!(error = %e, "core RPC server exited with error");
        }
    })
}
