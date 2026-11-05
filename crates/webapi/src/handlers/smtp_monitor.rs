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

/// The per-process fan-out for trace frames. One subscriber thread owns
/// the kevy connection; each WS client gets its own receiver.
pub type TraceBus = broadcast::Sender<String>;

/// Latest counter frame and when it arrived (unix seconds), or `None` if
/// no receiver has published since this webapi started. `None` is rendered
/// as a dash, never as zero — see [`ReceiverStats`].
#[derive(Default)]
pub struct StatsCache(std::sync::RwLock<Option<(ReceiverStats, i64)>>);

impl StatsCache {
    pub fn get(&self) -> Option<ReceiverStats> {
        self.0
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|(s, _)| s.clone()))
    }

    /// When the latest frame arrived here.
    pub fn received_at(&self) -> Option<i64> {
        self.0
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|(_, at)| *at))
    }

    fn set(&self, stats: ReceiverStats) {
        if let Ok(mut g) = self.0.write() {
            *g = Some((stats, chrono::Utc::now().timestamp()));
        }
    }
}

/// `GET /api/events/smtp` — upgrade to WS and stream the receiver's
/// protocol trace. Auth is done here rather than in the middleware, from
/// the same sources as `/api/events`. Administrators only: the trace is
/// every SMTP session on the server — peer addresses, envelope senders and
/// recipients of everybody's mail.
pub async fn ws_smtp_trace(
    ws: WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let (user, source) = crate::session::resolve_session_user(&headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !crate::session::websocket_origin_ok(&headers, source) {
        return Err(StatusCode::FORBIDDEN);
    }
    crate::handlers::kevy_util::require_super(&state, &user).await?;
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

/// The receiver's counters `/api/health` reports. They arrive on the
/// stats channel every ten seconds; null until a frame has come, which is
/// not zero. `sent_at` is when the receiver built the frame and
/// `received_at` when it came here: a frame that keeps its old times is the
/// last one before the receiver stopped.
pub fn health(stats: &StatsCache) -> serde_json::Value {
    match stats.get() {
        Some(s) => serde_json::json!({
            "probe_guard_tripped": s.probe_guard_tripped,
            "mask_submit_log_failed": s.mask_submit_log_failed,
            "dane": s.dane.as_ref().map(dane_health),
            "sent_at": s.sent_at,
            "received_at": stats.received_at(),
            "age_secs": stats.received_at().map(|at| chrono::Utc::now().timestamp() - at),
        }),
        None => serde_json::Value::Null,
    }
}

/// The DANE summary a watch reads: how many records are published (both
/// validating resolvers return them with AD set), whether the served
/// certificate and the next key are among them, and each server's answer.
fn dane_health(d: &mailrs_core_sidestate::smtp_monitor::DaneStatus) -> serde_json::Value {
    serde_json::json!({
        "name": d.name,
        "published": d.published().len(),
        "matches_current": d.matches(d.current.as_ref()),
        "matches_next": d.next.as_ref().map(|n| d.matches(Some(n))),
        "sources_agree": d.sources_agree(),
        "checked_at": d.checked_at,
        "unmatched_swaps": d.unmatched_swaps,
        "problem": dane_problem(d),
        "sources": d.sources,
    })
}

/// Something to act on: records are published and the served certificate
/// is not among them, or a certificate change has already gone to a key
/// nothing published. Nothing published yet is not a problem, and none of
/// this changes the overall health status.
fn dane_problem(d: &mailrs_core_sidestate::smtp_monitor::DaneStatus) -> bool {
    let published = !d.published().is_empty();
    (published && !d.matches(d.current.as_ref())) || d.unmatched_swaps > 0
}

/// TTL for the TLSA lines printed below. A key is swapped only after its
/// record has been published for at least two of these.
const TLSA_TTL_SECS: u32 = 3600;

/// `GET /api/admin/dane/tlsa` — the TLSA records port 25 needs, as zone
/// lines: the served certificate's, then the next key's when one is
/// configured. Super administrators only (`route_permissions`); 503
/// until the receiver has reported.
pub async fn dane_tlsa(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<String, StatusCode> {
    let dane = state
        .receiver_stats
        .get()
        .and_then(|s| s.dane)
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(tlsa_lines(&dane))
}

fn tlsa_lines(d: &mailrs_core_sidestate::smtp_monitor::DaneStatus) -> String {
    [d.current.as_ref(), d.next.as_ref()]
        .into_iter()
        .flatten()
        .map(|r| format!("{}. {TLSA_TTL_SECS} IN TLSA {r}\n", d.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dane(next: Option<&str>) -> mailrs_core_sidestate::smtp_monitor::DaneStatus {
        use mailrs_core_sidestate::smtp_monitor::{DaneSource, DaneStatus};
        let src = |server: &str, ad| DaneSource {
            server: server.into(),
            authenticated: ad,
            records: vec!["3 1 1 aa".into()],
            error: None,
        };
        DaneStatus {
            name: "_25._tcp.mx.example".into(),
            current: Some("3 1 1 aa".into()),
            next: next.map(String::from),
            checked_at: Some(1_762_000_000),
            sources: vec![
                src("1.1.1.1", Some(true)),
                src("8.8.8.8", Some(true)),
                src("ns.example", None),
            ],
            unmatched_swaps: 0,
        }
    }

    #[test]
    fn the_tlsa_lines_name_the_current_key_then_the_next() {
        assert_eq!(
            tlsa_lines(&dane(Some("3 1 1 bb"))),
            "_25._tcp.mx.example. 3600 IN TLSA 3 1 1 aa\n\
             _25._tcp.mx.example. 3600 IN TLSA 3 1 1 bb\n"
        );
        assert_eq!(
            tlsa_lines(&dane(None)),
            "_25._tcp.mx.example. 3600 IN TLSA 3 1 1 aa\n"
        );
    }

    #[test]
    fn dane_health_counts_published_records_and_says_which_keys_they_cover() {
        let h = dane_health(&dane(Some("3 1 1 bb")));
        assert_eq!(h["published"], 1);
        assert_eq!(h["matches_current"], true);
        assert_eq!(h["matches_next"], false);
        assert_eq!(h["sources_agree"], true);
        assert_eq!(h["sources"].as_array().unwrap().len(), 3);
        assert!(dane_health(&dane(None))["matches_next"].is_null());
        assert_eq!(h["problem"], false);
    }

    #[test]
    fn dane_is_a_problem_only_once_records_exist_and_miss_or_a_swap_missed() {
        let mut nothing = dane(None);
        for s in &mut nothing.sources {
            s.records.clear();
            s.authenticated = s.authenticated.map(|_| false);
        }
        let h = dane_health(&nothing);
        assert_eq!(h["published"], 0);
        assert_eq!(h["matches_current"], false);
        assert_eq!(
            h["problem"], false,
            "nothing published yet is not a problem"
        );

        let mut stale = dane(None);
        stale.current = Some("3 1 1 cc".into());
        assert_eq!(
            dane_health(&stale)["problem"],
            true,
            "published, served key not among them"
        );

        let mut swapped = dane(None);
        swapped.unmatched_swaps = 1;
        assert_eq!(dane_health(&swapped)["matches_current"], true);
        assert_eq!(
            dane_health(&swapped)["problem"],
            true,
            "a swap already went to an unpublished key"
        );
    }

    #[test]
    fn a_resolver_that_timed_out_neither_validates_nor_breaks_agreement() {
        let mut d = dane(None);
        d.sources[1].error = Some("timed out".into());
        d.sources[1].authenticated = Some(false);
        d.sources[1].records.clear();
        let h = dane_health(&d);
        assert_eq!(h["published"], 0, "one validating resolver did not answer");
        assert_eq!(
            h["sources_agree"], true,
            "only the servers that answered are compared"
        );
        assert_eq!(h["problem"], false);
    }

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
            probe_guard_tripped: Some(0),
            mask_submit_log_failed: None,
            dane: None,
            parsing: None,
            sent_at: Some(1_700_000_000),
        })
        .unwrap();

        dispatch(STATS_CHANNEL, &frame, &tx, &cache);

        assert_eq!(cache.get().unwrap().total_connections, 483);
        let received = cache.received_at().expect("arrival time kept");
        assert!((chrono::Utc::now().timestamp() - received).abs() < 5);
        assert_eq!(health(&cache)["sent_at"], 1_700_000_000);
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
