//! mailrs-core-api HTTP RPC server, hosted inside the monolith.
//!
//! Phase 2 — checklist `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §2.
//!
//! This module is compiled ONLY when the `core-rpc` cargo feature is on.
//! Without that feature the file is `#[cfg]`-skipped and the monolith
//! produces a byte-identical artifact (ironrule §16 守护点).
//!
//! What this module does:
//!
//! 1. Wraps `Arc<PgMailboxStore>` + `Arc<DomainStore>` + the rest of
//!    server's state into a `Handler` impl from `mailrs_core_api::server`.
//! 2. Mounts the per-method routes onto an `axum::Router`.
//! 3. Binds on `MAILRS_CORE_RPC_ADDR` (defaults to `0.0.0.0:3300`).
//! 4. Verifies `Authorization: Bearer <MAILRS_CORE_API_SECRET>` on
//!    authenticated routes.
//!
//! What this module does NOT do (yet):
//!
//! - Per-method handler bodies — for now only `/v1/healthz` + `/v1/readyz`
//!   are implemented. Per-method routes fill in over subsequent loops
//!   (checklist 2.2).
//! - Touch any existing pg/spg code path; all reads/writes still go via
//!   the existing `Arc<PgMailboxStore>` methods.

#![cfg(feature = "core-rpc")]

mod handlers;
mod router;
pub(crate) use router::build_full_router;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};

use mailrs_core_api::server::Handler;
use mailrs_core_api::types::{BackendKind, HealthResponse};

/// Aggregate of all state the core RPC server needs to answer requests.
///
/// Cement passes one of these in to `spawn_core_rpc`. The shape mirrors
/// the future `mailrs-core` binary's state struct.
///
/// Fields are `#[allow(dead_code)]` because checklist 2.1 only mounts
/// healthz/readyz; per-method handlers (2.2) will read them.
#[allow(dead_code)]
pub struct CoreRpcState {
    pub mailbox: Arc<mailrs_mailbox::PgMailboxStore>,
    pub domain: Arc<crate::domain_store::DomainStore>,
    /// Direct pool handle — used by `api_key_store` free functions and
    /// other admin paths that don't go through DomainStore.
    pub pool: crate::pg::BackendPool,
    /// Maildir root — handlers that serve raw bytes look up the file
    /// under `{maildir_root}/{user}/cur|new/{maildir_id}`.
    pub maildir_root: String,
    /// Network-kevy URL (`MAILRS_KEVY_URL`) for the SHARED side-state
    /// families (drafts/signatures/templates/reactions/webhooks/audit/
    /// contacts/analysis/outbound/groups/api-keys/sieve). The pg-core
    /// serves these from the same network kevy fastcore + webapi use, so
    /// the two cores' side-state routes are byte-identical. `None` when
    /// unset (side-state routes then serve empty).
    pub net_url: Option<String>,
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
            // Today's monolith is the "core" backend (PG/SPG-backed).
            // `fastcore` would set this to `Kevy`.
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
/// `MAILRS_CORE_API_SECRET` (default empty) — shared bearer secret for
/// authenticated routes. Empty secret means **no auth**, intended for
/// local dev only — production deploys MUST set it.
///
/// Returns immediately; the server runs in a background tokio task.
pub fn spawn_core_rpc(state: Arc<CoreRpcState>, shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    let addr = std::env::var("MAILRS_CORE_RPC_ADDR")
        .unwrap_or_else(|_| format!("0.0.0.0:{}", mailrs_core_api::DEFAULT_CORE_RPC_PORT));

    let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV).unwrap_or_default();
    if secret.is_empty() {
        tracing::warn!(
            event = "core_rpc_no_auth",
            "MAILRS_CORE_API_SECRET unset — core RPC will accept unauthenticated requests"
        );
    }

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
    });
}
