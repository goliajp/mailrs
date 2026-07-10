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
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::WebState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

/// Shared broadcast bus — one subscriber owns the kevy net client's
/// blocking subscribe loop; per-WS handlers each own a broadcast::Receiver.
/// Held on WebState as `Arc<OnceLock<broadcast::Sender<String>>>` lazily
/// initialized on the first WS upgrade.
pub type EventBus = broadcast::Sender<String>;

/// `GET /api/events?token=<hex>` — upgrade to WS, then stream kevy
/// pubsub frames. Auth is done here (not in middleware) because
/// browser WebSockets can only pass credentials via query string
/// or cookie, and the frontend uses `?token=`.
pub async fn ws_events(
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<WsQuery>,
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let token = query.token.as_deref().ok_or(StatusCode::UNAUTHORIZED)?;
    // Verify the session exists in shared kevy — same key the auth
    // middleware reads.
    let kevy_url =
        std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("session:{token}");
    let has_session = tokio::task::spawn_blocking(move || -> std::io::Result<bool> {
        let mut c = kevy_client::Connection::open(&kevy_url)?;
        Ok(c.get(key.as_bytes())?.is_some())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !has_session {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let bus = get_or_init_bus(state.clone()).await;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, bus)))
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
            // v2.3 §P7-B (2026-07-10): DUAL consumer. The legacy pubsub
            // subscriber stays live — it still fires today because
            // producers publish to `notify:new-mail`, and it's the only
            // path that has been proven in production. In parallel we
            // stand up the kevy 3.17 change-feed consumer, which reads
            // the SET frames producers dual-write per §P7-A. Both feed
            // the same broadcast::channel; frontend RQ layer treats a
            // duplicate "nudge to refetch" as a no-op, so the temporary
            // double-fire during migration is safe. §P7-C removes the
            // legacy publish + this pubsub subscriber together, ~1-2
            // weeks after this consumer has proven itself.
            spawn_kevy_subscriber(state.clone(), tx.clone());
            spawn_kevy_feed_consumers(tx.clone());
            tx
        }
        Err(_) => state.event_bus.get().unwrap().clone(),
    }
}

fn spawn_kevy_feed_consumers(tx: EventBus) {
    let Some(kevy_url) = std::env::var("MAILRS_KEVY_URL").ok() else {
        return;
    };
    // Discover shard count in a scratch connection. If the discovery
    // itself fails the migration path is a no-op — pubsub is still up.
    let shards = match kevy_client::Connection::open(&kevy_url) {
        Ok(mut c) => c.feed_shards().unwrap_or(1),
        Err(e) => {
            tracing::warn!(err = %e, url = %kevy_url, "feed_shards probe failed; skipping feed consumer");
            return;
        }
    };
    tracing::info!(shards, "spawning kevy feed consumers");
    for shard in 0..shards {
        let tx = tx.clone();
        let kevy_url = kevy_url.clone();
        tokio::task::spawn_blocking(move || feed_consumer_loop(&kevy_url, shard, tx));
    }
}

fn feed_consumer_loop(kevy_url: &str, shard: usize, tx: EventBus) {
    const PREFIX: &[u8] = b"mailrs:events:notify:";
    const IDLE_SLEEP: std::time::Duration = std::time::Duration::from_millis(250);
    const RECONNECT_SLEEP: std::time::Duration = std::time::Duration::from_secs(5);
    'outer: loop {
        let mut conn = match kevy_client::Connection::open(kevy_url) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(err = %e, shard, "feed consumer connect failed; retry 5s");
                std::thread::sleep(RECONNECT_SLEEP);
                continue;
            }
        };
        let (mut generation, mut off) = match conn.feed_tail(shard) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(err = %e, shard, "feed_tail failed; retry 5s");
                std::thread::sleep(RECONNECT_SLEEP);
                continue;
            }
        };
        tracing::info!(shard, generation, off, "feed consumer online");
        loop {
            let batch = match conn.feed_read(shard, generation, off, Some(256), &[PREFIX]) {
                Ok(b) => b,
                Err(e) => {
                    // FeedError::Resync surfaces as an io::Error whose
                    // Display contains the wire verb "FEEDRESYNC".
                    // Recover by re-tailing so we skip the invalidated
                    // window; the frontend refetches full state on the
                    // next WS reconnect, so brief resync gaps are
                    // acceptable.
                    if e.to_string().contains("FEEDRESYNC") {
                        tracing::info!(shard, "feed resync; re-tailing");
                        continue 'outer;
                    }
                    tracing::warn!(err = %e, shard, "feed_read failed; reconnect 5s");
                    std::thread::sleep(RECONNECT_SLEEP);
                    continue 'outer;
                }
            };
            generation = batch.generation;
            off = batch.next_offset;
            if batch.frames.is_empty() {
                std::thread::sleep(IDLE_SLEEP);
                continue;
            }
            for frame in batch.frames {
                // Only SET frames carry a publishable payload — EXPIRE /
                // DEL / etc. also match the prefix but their argv layout
                // has no value at position 2.
                let Some(verb) = frame.argv.first() else { continue };
                if verb.as_slice() != b"SET" {
                    continue;
                }
                let Some(value) = frame.argv.get(2) else { continue };
                let Ok(msg) = std::str::from_utf8(value) else { continue };
                // Shape unchanged from pubsub — same JSON envelope the
                // frontend `use-mail-events.ts` parses today.
                let _ = tx.send(msg.to_string());
            }
        }
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
