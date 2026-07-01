//! `/api/events` — real-time inbox updates via WebSocket.
//!
//! Phase 11. Subscribes to the shared network kevy-server pubsub
//! channel `notify:new-mail` (where monolith mailrs receiver publishes
//! `SpoolDelivered` / `MailIndexed` envelopes) and forwards each JSON
//! frame to WS clients over `/api/events`.
//!
//! Auth: uses the session-auth middleware — a valid session cookie
//! or bearer token is required just like every other authenticated
//! route. The upgrade request goes through the same middleware
//! stack, so a missing/invalid session gets a 401 before the WS
//! upgrade completes.
//!
//! Fan-out: one kevy subscription per webapi process (cheap; kevy
//! handles broadcast internally); each WS client gets its own tokio
//! mpsc to the shared subscriber loop. Clean up on disconnect.

use std::sync::Arc;

use axum::extract::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

use crate::WebState;

/// Shared broadcast bus — one subscriber owns the kevy net client's
/// blocking subscribe loop; per-WS handlers each own a broadcast::Receiver.
/// Held on WebState as `Arc<OnceLock<broadcast::Sender<String>>>` lazily
/// initialized on the first WS upgrade.
pub type EventBus = broadcast::Sender<String>;

/// `GET /api/events` — upgrade to WS, then stream kevy pubsub frames.
pub async fn ws_events(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> impl IntoResponse {
    let bus = get_or_init_bus(state.clone()).await;
    ws.on_upgrade(move |socket| handle_ws(socket, bus))
}

async fn handle_ws(socket: WebSocket, bus: EventBus) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = bus.subscribe();

    // forward events → WS
    let send_task = tokio::spawn(async move {
        while let Ok(text) = rx.recv().await {
            if sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // drain incoming (client → server) — ignored, keeps the socket alive
    let recv_task = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

/// Get the shared broadcast::Sender, initializing (+ spawning the kevy
/// subscribe loop) if this is the first caller.
async fn get_or_init_bus(state: Arc<WebState>) -> EventBus {
    if let Some(existing) = state.event_bus.get() {
        return existing.clone();
    }
    let (tx, _rx) = broadcast::channel::<String>(256);
    // Best-effort insert — if another task raced ahead, use their bus.
    match state.event_bus.set(tx.clone()) {
        Ok(()) => {
            spawn_kevy_subscriber(state, tx.clone());
            tx
        }
        Err(_) => state.event_bus.get().unwrap().clone(),
    }
}

fn spawn_kevy_subscriber(state: Arc<WebState>, tx: EventBus) {
    let Some(kevy_url) = std::env::var("MAILRS_KEVY_URL").ok() else {
        tracing::warn!("MAILRS_KEVY_URL unset — WS /api/events won't receive live events");
        return;
    };
    // Ignore compiler warning if state unused — the state Arc is here
    // for symmetry with future filtering hooks (per-user event streams).
    let _ = state;

    tokio::task::spawn_blocking(move || {
        loop {
            let mut sub = match kevy_client::Subscriber::open(&kevy_url, &[b"notify:new-mail"]) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, url = %kevy_url, "kevy subscribe open failed; retry 5s");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
            };
            tracing::info!("WS event bus subscribed to notify:new-mail");
            loop {
                match sub.recv_message() {
                    Ok((_channel, payload)) => {
                        let msg = String::from_utf8_lossy(&payload).to_string();
                        // slow subscribers drop; broadcast returns Err
                        // when nobody's listening — that's fine.
                        let _ = tx.send(msg);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "kevy recv error; reconnecting in 5s");
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        break;
                    }
                }
            }
        }
    });
}
