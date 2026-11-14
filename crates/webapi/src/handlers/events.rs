//! `/api/events` — real-time inbox updates via WebSocket.
//!
//! Forwards the frames published on `mailrs:events`
//! (`mailrs_core_sidestate::keys::events`). One subscriber per process;
//! a frame published while it is reconnecting is missed, and a client
//! refetches everything when its own socket reconnects.
//!
//! Auth: checked in the handler, from the same token sources as the
//! session middleware (the browser sends its session cookie on a
//! same-origin upgrade). A missing or invalid session gets a 401 before
//! the upgrade completes.
//!
//! The subscriber's connection is its own, not pooled: a subscribed
//! connection takes no other commands.
//!
//! Fan-out: each WS client gets its own receiver on the shared broadcast
//! bus, and is sent only the events addressed to the session's own user.
//! Clean up on disconnect.

use std::sync::Arc;

use axum::extract::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

use crate::WebState;

/// Shared broadcast bus — one subscriber owns the kevy net client's
/// blocking subscribe loop; per-WS handlers each own a broadcast::Receiver.
/// Held on WebState as `Arc<OnceLock<broadcast::Sender<String>>>` lazily
/// initialized on the first WS upgrade.
pub type EventBus = broadcast::Sender<String>;

/// `GET /api/events` — upgrade to WS, then stream kevy feed frames.
/// Auth is done here, not in the middleware, but from the same sources:
/// the browser sends the session cookie on a same-origin upgrade.
pub async fn ws_events(
    ws: WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let (address, source) = crate::session::resolve_session_user(&headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !crate::session::websocket_origin_ok(&headers, source) {
        return Err(StatusCode::FORBIDDEN);
    }
    let user = normalized_user(&address).ok_or(StatusCode::UNAUTHORIZED)?;
    let held = crate::session::grant::Held::from_headers(&user, &headers)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let bus = get_or_init_bus(state.clone()).await;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, bus, user, state, held)))
}

/// The address events are matched against.
fn normalized_user(address: &str) -> Option<String> {
    let address = address.trim();
    (!address.is_empty()).then(|| address.to_ascii_lowercase())
}

/// Whether an event on the bus is addressed to `user` (lowercased).
///
/// The bus carries every user's events, and it was forwarded whole to
/// every socket: each signed-in browser received the sender and subject
/// of everybody's new mail and dropped what was not its own. An event
/// with no recipient goes to nobody.
pub(crate) fn addressed_to(event: &str, user: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(event)
        .ok()
        .and_then(|v| {
            v.get("user")?
                .as_str()
                .map(|u| u.trim().eq_ignore_ascii_case(user))
        })
        .unwrap_or(false)
}

/// Forward this user's events until the socket closes, or until what
/// signed it in no longer would (signed out, password changed, account
/// stopped): asked every `RECHECK`.
async fn handle_ws(
    socket: WebSocket,
    bus: EventBus,
    user: String,
    state: Arc<WebState>,
    held: crate::session::grant::Held,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = bus.subscribe();
    let mut recheck = tokio::time::interval(crate::session::grant::RECHECK);
    recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    recheck.reset();
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(text) => {
                    if addressed_to(&text, &user)
                        && sender.send(Message::Text(text.into())).await.is_err()
                    {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            // client → server frames are ignored; they keep it alive
            msg = receiver.next() => match msg {
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
            _ = recheck.tick() => {
                if !held.still_valid(&state).await {
                    tracing::info!(%user, "events socket: the credential no longer holds, closing");
                    let frame = axum::extract::ws::CloseFrame { code: 1008, reason: "signed out".into() };
                    let _ = sender.send(Message::Close(Some(frame))).await;
                    return;
                }
            }
        }
    }
}

/// Get the shared broadcast::Sender, initializing (+ spawning the kevy
/// subscribe loop) if this is the first caller.
pub(crate) async fn get_or_init_bus(state: Arc<WebState>) -> EventBus {
    if let Some(existing) = state.event_bus.get() {
        return existing.clone();
    }
    let (tx, _rx) = broadcast::channel::<String>(256);
    // Best-effort insert — if another task raced ahead, use their bus.
    match state.event_bus.set(tx.clone()) {
        Ok(()) => {
            let _ = state;
            spawn_subscriber(tx.clone());
            tx
        }
        Err(_) => state.event_bus.get().unwrap().clone(),
    }
}

fn spawn_subscriber(tx: EventBus) {
    use mailrs_core_sidestate::keys::events::EVENTS_CHANNEL;
    const RECONNECT: std::time::Duration = std::time::Duration::from_secs(1);
    let Some(url) = std::env::var("MAILRS_KEVY_URL").ok() else {
        return;
    };
    std::thread::Builder::new()
        .name("live-events".into())
        .spawn(move || {
            loop {
                match kevy_client::Subscriber::connect_channels(&url, &[EVENTS_CHANNEL]) {
                    Ok(mut sub) => {
                        tracing::info!("live events: subscribed");
                        while let Ok((_, frame)) = sub.recv_message() {
                            if let Ok(msg) = String::from_utf8(frame) {
                                let _ = tx.send(msg);
                            }
                        }
                        tracing::warn!("live events: subscription ended; reconnecting");
                    }
                    Err(e) => tracing::warn!(err = %e, "live events: subscribe failed; retry 1s"),
                }
                std::thread::sleep(RECONNECT);
            }
        })
        .expect("spawn live-events thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_reaches_only_its_own_user() {
        let ev = r#"{"type":"NewMessage","user":"Bob@Golia.jp","subject":"s"}"#;
        assert!(addressed_to(ev, "bob@golia.jp"));
        assert!(!addressed_to(ev, "alice@golia.jp"));
        assert!(!addressed_to(
            r#"{"type":"NewMessage","subject":"s"}"#,
            "bob@golia.jp"
        ));
        assert!(!addressed_to("not json", "bob@golia.jp"));
    }

    #[test]
    fn a_mailbox_change_reaches_only_its_own_user() {
        let frame =
            mailrs_core_sidestate::families::live_events::mailbox_changed_frame("Bob@Golia.jp");
        let ev = std::str::from_utf8(&frame).unwrap();
        assert!(addressed_to(ev, "bob@golia.jp"));
        assert!(!addressed_to(ev, "alice@golia.jp"));
    }

    #[test]
    fn a_session_names_its_user() {
        assert_eq!(
            normalized_user(" Bob@Golia.jp ").as_deref(),
            Some("bob@golia.jp")
        );
        assert_eq!(normalized_user(""), None);
    }
}
