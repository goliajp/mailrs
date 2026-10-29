//! Bringing the process up: listeners, background tasks, and the shutdown
//! that has to finish before the kevy AOF is safe to replay.
//!
//! The graceful stop is not decoration. A hard kill mid-write leaves a
//! torn frame at the AOF tail; replay stops there while appends continue
//! past it, so every restart rolls the store back to the moment of the
//! tear and new writes vanish, which is why the replay log is checked for
//! `(clean)` after every deploy.

use std::sync::Arc;

use crate::*;

mod store;
mod tasks;

pub async fn run() {
    // Install the process-wide rustls crypto provider before any TLS
    // config is built (IMAPS / POP3S acceptors, ACME challenge server).
    // Without this rustls 0.23 panics on first use — same fix
    // mailrs-receiver / mailrs-fastcore-sender apply. `.ok()` because
    // a second install is a no-op error we can safely ignore.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // before the store is opened: a replay is minutes of work to throw away
    if let Err(e) = mailrs_core_api::required_auth_secret() {
        tracing::error!("{e}");
        std::process::exit(2);
    }

    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let state = store::open_state(&kevy_dir);
    // Held for the shutdown path — the router takes ownership of the
    // original below.
    let shutdown_state = state.clone();

    tasks::spawn_workers(&state, &kevy_dir);
    tasks::spawn_listeners(&state);
    tasks::spawn_outbound_feeds(&state);

    let addr = std::env::var("MAILRS_FASTCORE_BIND").unwrap_or_else(|_| "0.0.0.0:3301".into());

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind MAILRS_FASTCORE_BIND");
    tracing::info!(
        addr = %addr,
        kevy_dir = %kevy_dir,
        "mailrs-fastcore listening (kevy backend)"
    );
    // Exit gracefully on SIGTERM/SIGINT instead of letting the default
    // handler kill the process mid-write. Returning from run() drops the
    // runtime → every task's Arc<Store> releases → kevy's DropGuard
    // flushes each shard's AOF. Without this, `docker stop` (every
    // deploy) tore a half-written frame into the AOF tail and the next
    // boot's replay DROPPED everything after it — 181 MB / several days
    // of writes on 2026-07-17 (vanished mail, resurrected threading
    // fragments).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        r = axum::serve(listener, app) => {
            r.unwrap();
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM — flushing kevy before exit");
            flush_kevy(&shutdown_state);
            exit_after_flush();
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("SIGINT — flushing kevy before exit");
            flush_kevy(&shutdown_state);
            exit_after_flush();
        }
    }
}

/// Leave, now that the store is sealed.
///
/// Returning from here instead would drop the runtime, and dropping a
/// runtime waits for its blocking tasks — of which this process has
/// several that loop forever on a bounded `BRPOP`, by design. So the
/// process flushed, said so, and then never exited: measured still alive
/// 40 s after SIGTERM whenever `MAILRS_KEVY_URL` was set, which is the
/// production configuration. Every deploy was waiting out `docker stop`'s
/// grace period and taking a SIGKILL.
///
/// This is safe precisely because `flush_kevy` ran first: kevy's
/// `shutdown()` fsyncs every shard's AOF and then refuses further writes
/// with `KevyError::Closed`, so there is no write left in flight for a
/// skipped destructor to lose. Maildir writes are closed files. The
/// network kevy is somebody else's process.
fn exit_after_flush() -> ! {
    tracing::info!("exiting");
    std::process::exit(0);
}

/// Flush and seal the store on the way out.
///
/// The previous version relied on returning from `run()` dropping the
/// runtime, which released every task's `Arc<Store>`, which let kevy's
/// DropGuard flush. That works only if nothing else still holds a
/// clone — a race the code could not actually prove it won.
///
/// kevy 4.0 makes it explicit: `shutdown()` fsyncs everything and then
/// refuses further writes with `KevyError::Closed`, and it is
/// clone-safe, so it does not matter who else is holding the store.
/// 4.0 also force-fsyncs the AOF tail before writing the feed's
/// clean-shutdown marker — the marker could previously claim a
/// durability the tail did not have.
fn flush_kevy(state: &Arc<FastcoreState>) {
    match state.mailbox.store_ref().shutdown() {
        Ok(()) => tracing::info!("kevy shutdown complete — AOF fsynced, writes refused"),
        Err(e) => tracing::error!(error = %e, "kevy shutdown failed; AOF tail may be unflushed"),
    }
}
