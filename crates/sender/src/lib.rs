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

    // Phase 4.3 — claim-loop scaffold. Periodically polls
    // /v1/outbound/claim. SMTP delivery + mark_delivered land in a
    // subsequent loop; today this just exercises the RPC + logs depth so
    // staging operators see the channel working.
    let claim_client = Arc::clone(&state.core_client);
    tokio::spawn(claim_loop(claim_client));

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

/// Periodic claim loop — polls core's outbound_claim every CLAIM_TICK
/// seconds. Phase 4.3 scaffold: logs claim count + queue depth, does
/// NOT yet emit SMTP or mark deliveries (lands in 4.4–4.9 once the
/// mailrs-smtp-client integration is wired through state).
async fn claim_loop(core_client: Arc<mailrs_core_api::client::Client>) {
    const CLAIM_TICK_SECS: u64 = 5;
    const BATCH_SIZE: u32 = 16;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(CLAIM_TICK_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        match core_client.outbound_claim(BATCH_SIZE).await {
            Ok(resp) if !resp.items.is_empty() => {
                tracing::info!(
                    claimed = resp.items.len(),
                    "outbound claim got messages (delivery NYI — Phase 4.4)"
                );
                // TODO(checklist 4.4): for each item:
                //   1. relay via mailrs_smtp_client with DKIM sign
                //   2. on success: core_client.mark_delivered(item.id)
                //   3. on failure: core_client.mark_failed(item.id, ...)
                //   4. on hard bounce: core_client.add_suppression(...)
            }
            Ok(_) => {
                tracing::debug!("outbound claim — queue empty");
            }
            Err(mailrs_core_api::error::CoreApiError::Unauthorized) => {
                tracing::error!(
                    "outbound claim 401 — MAILRS_CORE_API_SECRET mismatch; \
                     loop continues to allow secret rotation"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "outbound claim failed");
            }
        }
    }
}
