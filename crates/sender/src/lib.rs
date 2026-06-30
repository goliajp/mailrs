//! mailrs-sender — outbound delivery + webhook + DMARC report worker.
//!
//! Phase 4 of the 4-process split (checklist
//! `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §4).
//!
//! Today this crate is a **scaffold**: it boots a `mailrs-core-api`
//! client, probes core liveness, and idles waiting for the worker loops
//! to fill in. The outbound-queue and webhook workers currently live
//! inside the monolith (`crates/server/src/bootstrap/outbound.rs` +
//! `crates/server/src/webhook/`). When checklist 4.3–4.9 move them here,
//! they will:
//!
//! 1. Periodically `claim_for_delivery` via RPC instead of the local PG pool
//! 2. Build the SMTP envelope locally (no DB access)
//! 3. Call `mark_delivered` / `mark_failed` / etc back to core via RPC
//!
//! Boot order:
//! - tracing init
//! - config from env (MAILRS_CORE_RPC_BASE / MAILRS_CORE_API_SECRET)
//! - mailrs-core-api client
//! - core healthz probe
//! - signal handler (no worker loops yet)

#![allow(missing_docs)]

use std::sync::Arc;

pub struct SenderState {
    pub core_client: Arc<mailrs_core_api::client::Client>,
}

impl SenderState {
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for sender");
        let core_client = Arc::new(mailrs_core_api::client::Client::new(base, secret));
        Self { core_client }
    }
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let state = Arc::new(SenderState::from_env());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "mailrs-sender starting (Phase 4 scaffold — worker loops pending)"
    );

    match state.core_client.healthz().await {
        Ok(h) => {
            tracing::info!(
                version = %h.version,
                backend = ?h.backend,
                "core RPC reachable — sender ready (workers idle)"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "core RPC unreachable at startup — sender will retry"
            );
        }
    }

    // Idle until SIGTERM / Ctrl-C. Worker loops mount onto this state
    // over checklist 4.3–4.9.
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.expect("ctrl_c"),
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("ctrl_c");

    tracing::info!("mailrs-sender shutting down");
}
