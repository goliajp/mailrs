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

mod deliver;

use std::sync::Arc;

pub struct SenderState {
    pub core_client: Arc<mailrs_core_api::client::Client>,
    pub resolver: Arc<hickory_resolver::TokioResolver>,
    pub hostname: String,
}

impl SenderState {
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for sender");
        let core_client = Arc::new(mailrs_core_api::client::Client::new(base, secret));
        let resolver = Arc::new(
            hickory_resolver::TokioResolver::builder_tokio()
                .expect("TokioResolver builder")
                .build()
                .expect("TokioResolver build"),
        );
        let hostname =
            std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mailrs-sender.local".into());
        Self {
            core_client,
            resolver,
            hostname,
        }
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
    tokio::spawn(claim_loop(Arc::clone(&state)));

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
async fn claim_loop(state: Arc<SenderState>) {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    const CLAIM_TICK_SECS: u64 = 5;
    const BATCH_SIZE: u32 = 16;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(CLAIM_TICK_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let resp = match state.core_client.outbound_claim(BATCH_SIZE).await {
            Ok(r) => r,
            Err(mailrs_core_api::error::CoreApiError::Unauthorized) => {
                tracing::error!("outbound claim 401 — secret mismatch (loop continues)");
                continue;
            }
            Err(e) => {
                tracing::warn!(error = %e, "outbound claim failed");
                continue;
            }
        };
        if resp.items.is_empty() {
            tracing::debug!("outbound claim — queue empty");
            continue;
        }
        tracing::info!(claimed = resp.items.len(), "delivering outbound batch");

        for item in &resp.items {
            let body = match B64.decode(item.message_data_base64.as_bytes()) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, id = item.id, "base64 decode failed; bouncing");
                    let _ = state
                        .core_client
                        .outbound_mark_failed(item.id, format!("base64: {e}"))
                        .await;
                    continue;
                }
            };
            match deliver::deliver_envelope(
                &state.resolver,
                &item.sender,
                &item.recipient,
                &body,
                &state.hostname,
            )
            .await
            {
                Ok(deliver::Outcome::Accepted) => {
                    tracing::info!(id = item.id, recipient = %item.recipient, "delivered");
                    if let Err(e) = state.core_client.outbound_mark_delivered(item.id).await {
                        tracing::warn!(error = %e, id = item.id, "mark_delivered RPC failed");
                    }
                }
                Ok(deliver::Outcome::Transient(msg)) => {
                    tracing::info!(id = item.id, recipient = %item.recipient, %msg, "transient");
                    let _ = state.core_client.outbound_mark_failed(item.id, msg).await;
                }
                Ok(deliver::Outcome::Permanent(msg)) => {
                    tracing::warn!(id = item.id, recipient = %item.recipient, %msg, "permanent");
                    let _ = state.core_client.outbound_mark_failed(item.id, msg).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, id = item.id, "deliver transport failed");
                    let _ = state
                        .core_client
                        .outbound_mark_failed(item.id, format!("transport: {e}"))
                        .await;
                }
            }
        }
    }
}
