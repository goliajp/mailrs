//! `/api/events/smtp` — the SMTP live monitor's stream, and the receiver
//! counters behind `/api/status`.
//!
//! **Why this is a second WebSocket and not `/api/events`.** That one is
//! the inbox feed: every phone and every open tab holds one, and a frame
//! on it means "your mail changed". A protocol trace is diagnostic, is
//! wanted only while somebody is looking at the monitor, and would
//! otherwise be delivered to clients that have no use for it.
//!
//! **Why pub/sub and not the change feed.** The inbox feed is durable
//! because a missed `NewMessage` is a message the reader never sees. A
//! missed `CommandReceived` is one line of a trace that has already
//! scrolled; making it durable would write tens of thousands of AOF
//! entries a day for a page that is usually closed.
//!
//! Before this existed the page rendered `use-smtp-events` against
//! `/api/events`, which carries a different event vocabulary entirely —
//! so it reported "connected" and showed nothing, for as long as the
//! four-process split has existed. The reader half was complete; the
//! writer half stopped inside `mailrs-receiver`.

use std::sync::Arc;

use axum::extract::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use mailrs_core_sidestate::smtp_monitor::{ReceiverStats, STATS_CHANNEL, TRACE_CHANNEL};
use tokio::sync::broadcast;

use crate::WebState;
use crate::handlers::events::WsQuery;

/// The per-process fan-out for trace frames. One subscriber thread owns
/// the kevy connection; each WS client gets its own receiver.
pub type TraceBus = broadcast::Sender<String>;

/// Latest counter frame, or `None` if no receiver has published since
/// this webapi started. `None` is rendered as a dash, never as zero —
/// see [`ReceiverStats`].
#[derive(Default)]
pub struct StatsCache(std::sync::RwLock<Option<ReceiverStats>>);

impl StatsCache {
    pub fn get(&self) -> Option<ReceiverStats> {
        self.0.read().ok().and_then(|g| *g)
    }

    fn set(&self, stats: ReceiverStats) {
        if let Ok(mut g) = self.0.write() {
            *g = Some(stats);
        }
    }
}

/// `GET /api/events/smtp?token=<hex>` — upgrade to WS and stream the
/// receiver's protocol trace. Auth is done here rather than in
/// middleware for the same reason as `/api/events`: a browser
/// WebSocket cannot set a header.
pub async fn ws_smtp_trace(
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<WsQuery>,
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let token = query.token.as_deref().ok_or(StatusCode::UNAUTHORIZED)?;
    let kevy_url =
        std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("session:{token}");
    let has_session = tokio::task::spawn_blocking(move || -> std::io::Result<bool> {
        let mut c = kevy_client::Connection::connect(&kevy_url)?;
        Ok(c.get(key.as_bytes())?.is_some())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !has_session {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let bus = get_or_init_trace_bus(state);
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, bus)))
}

async fn handle_ws(socket: WebSocket, bus: TraceBus) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = bus.subscribe();
    let send_task = tokio::spawn(async move {
        while let Ok(text) = rx.recv().await {
            if sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let recv_task = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

/// Start the subscriber at boot.
///
/// It would be cheaper to start it on the first monitor connection, and
/// that is wrong for a reason that only shows up in the first thirteen
/// seconds: the same subscriber fills the counter cache `/api/status`
/// serves, so a lazy start would make a general endpoint's answer depend
/// on whether somebody had opened a particular page. The reader would
/// load the monitor, see four dashes, and get numbers only after a stats
/// tick — which is exactly the "why is this empty" this whole change
/// exists to answer.
///
/// The standing cost is one kevy connection and roughly one frame every
/// three seconds, dropped by `broadcast::send` when nobody is
/// subscribed.
pub fn spawn(state: &Arc<WebState>) {
    let _ = get_or_init_trace_bus(state.clone());
}

/// Get the bus, starting the subscriber if [`spawn`] has not run. The
/// lazy path is kept for callers that build a `WebState` without the
/// boot sequence — tests, and the smoke harness.
fn get_or_init_trace_bus(state: Arc<WebState>) -> TraceBus {
    if let Some(existing) = state.trace_bus.get() {
        return existing.clone();
    }
    let (tx, _rx) = broadcast::channel::<String>(1024);
    match state.trace_bus.set(tx.clone()) {
        Ok(()) => {
            spawn_trace_subscriber(tx.clone(), state.receiver_stats.clone());
            tx
        }
        Err(_) => state.trace_bus.get().expect("set raced").clone(),
    }
}

fn spawn_trace_subscriber(tx: TraceBus, stats: Arc<StatsCache>) {
    let Ok(kevy_url) = std::env::var("MAILRS_KEVY_URL") else {
        tracing::warn!("MAILRS_KEVY_URL unset; SMTP monitor will stay empty");
        return;
    };
    std::thread::Builder::new()
        .name("smtp-trace-subscriber".into())
        .spawn(move || trace_subscriber_loop(&kevy_url, &tx, &stats))
        .expect("spawn smtp-trace-subscriber thread");
}

fn trace_subscriber_loop(url: &str, tx: &TraceBus, stats: &StatsCache) {
    const RECONNECT: std::time::Duration = std::time::Duration::from_secs(1);
    loop {
        match kevy_client::Subscriber::connect_channels(url, &[TRACE_CHANNEL, STATS_CHANNEL]) {
            Ok(mut sub) => {
                tracing::info!("smtp trace subscriber online");
                while let Ok((channel, payload)) = sub.recv_message() {
                    dispatch(&channel, &payload, tx, stats);
                }
            }
            Err(e) => tracing::warn!(err = %e, "smtp trace subscribe failed; retry 1s"),
        }
        std::thread::sleep(RECONNECT);
    }
}

/// Route one frame by the channel it arrived on. Trace payloads are
/// forwarded **verbatim**: the receiver publishes a bare `SmtpEvent`, so
/// there is nothing here that has to know its shape and therefore
/// nothing here that can disagree with it.
fn dispatch(channel: &[u8], payload: &[u8], tx: &TraceBus, stats: &StatsCache) {
    if channel == STATS_CHANNEL {
        if let Ok(s) = serde_json::from_slice::<ReceiverStats>(payload) {
            stats.set(s);
        }
        return;
    }
    if channel == TRACE_CHANNEL
        && let Ok(text) = std::str::from_utf8(payload)
    {
        let _ = tx.send(text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unpublished_cache_reads_as_unknown_not_zero() {
        let cache = StatsCache::default();
        assert!(
            cache.get().is_none(),
            "no frame yet must not read as zeroes"
        );
    }

    #[test]
    fn a_stats_frame_lands_in_the_cache_and_not_on_the_wire() {
        let (tx, mut rx) = broadcast::channel::<String>(8);
        let cache = StatsCache::default();
        let frame = serde_json::to_vec(&ReceiverStats {
            active_connections: 1,
            total_connections: 483,
            total_messages: 17,
            uptime_secs: 60,
        })
        .unwrap();

        dispatch(STATS_CHANNEL, &frame, &tx, &cache);

        assert_eq!(cache.get().unwrap().total_connections, 483);
        // The counters are a REST answer, not a stream event — forwarding
        // them onto the trace socket would show up as an unparseable
        // entry in the event list.
        assert!(
            rx.try_recv().is_err(),
            "a stats frame must not be broadcast"
        );
    }

    #[test]
    fn a_trace_frame_goes_out_verbatim() {
        let (tx, mut rx) = broadcast::channel::<String>(8);
        let cache = StatsCache::default();
        let raw = br#"{"type":"ConnectionOpened","id":7,"addr":"1.2.3.4:25","tls":false}"#;

        dispatch(TRACE_CHANNEL, raw, &tx, &cache);

        assert_eq!(rx.try_recv().unwrap(), std::str::from_utf8(raw).unwrap());
        // …and it must not be mistaken for a counter frame.
        assert!(cache.get().is_none());
    }

    /// A frame on neither channel is dropped rather than guessed at.
    #[test]
    fn an_unknown_channel_is_ignored() {
        let (tx, mut rx) = broadcast::channel::<String>(8);
        let cache = StatsCache::default();
        dispatch(b"notify:new-mail", br#"{"origin":"x"}"#, &tx, &cache);
        assert!(rx.try_recv().is_err());
        assert!(cache.get().is_none());
    }
}
