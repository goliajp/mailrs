//! Cross-process event notification over a shared kevy-server.
//!
//! In the receiver-split topology, a process that delivers mail
//! publishes the user-facing events to a kevy pub/sub channel; other
//! processes (e.g. the core serving web / IMAP-IDLE clients) subscribe
//! and re-inject them into their in-process [`EventBus`]. This
//! *supplements* the in-process broadcast — it never replaces it.
//!
//! Reliability is intentionally best-effort: pub/sub is not durable, so
//! a subscriber that's offline drops messages. The low-frequency
//! reconcile pass (P2) is the durability backstop; notifications only
//! make the live UI feel instant.
//!
//! **Loop guard:** every process stamps published envelopes with its
//! own `origin` id and the subscriber skips its own — so a single
//! process can both publish and subscribe (the monolith, or any fleet
//! member) without echoing its events back to itself.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use kevy_client::Subscriber;

use crate::event_bus::{EventBus, SmtpEvent};
use crate::kevy_net::KevyNetClient;

/// The default pub/sub channel for mail notifications.
pub const NOTIFY_CHANNEL: &[u8] = b"notify:new-mail";

/// Wire envelope: an [`SmtpEvent`] plus the publishing process's
/// `origin` id (for the subscriber's self-skip loop guard).
#[derive(Serialize, Deserialize)]
struct NotifyEnvelope {
    origin: String,
    event: SmtpEvent,
}

/// A unique-per-process origin id: `<pid>-<start-nanos>`. Two processes
/// can't share a pid at the same start instant, so this is collision-
/// free in practice without pulling in a uuid dependency.
pub fn process_origin() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), nanos)
}

/// Publishes cross-process-worthy events to a shared kevy-server.
/// Held by the [`EventBus`]; `publish` is fire-and-forget so a slow or
/// unreachable server never blocks the emitting session.
pub struct KevyEventPublisher {
    client: Arc<KevyNetClient>,
    channel: Vec<u8>,
    origin: String,
}

impl KevyEventPublisher {
    pub fn new(client: Arc<KevyNetClient>, channel: Vec<u8>, origin: String) -> Self {
        Self {
            client,
            channel,
            origin,
        }
    }

    /// Publish `event` to the channel, stamped with this process's
    /// origin. Spawns the blocking RESP publish onto a background task
    /// and returns immediately; publish failures are dropped (the
    /// reconcile pass backstops durability).
    pub fn publish(&self, event: &SmtpEvent) {
        let envelope = NotifyEnvelope {
            origin: self.origin.clone(),
            event: event.clone(),
        };
        let json = match serde_json::to_vec(&envelope) {
            Ok(j) => j,
            Err(_) => return,
        };
        let client = self.client.clone();
        let channel = self.channel.clone();
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                client.with_conn(|c| c.publish(&channel, &json).map(|_| ()))
            })
            .await;
        });
    }
}

/// Spawn the subscriber bridge: a dedicated OS thread that subscribes
/// to `channel` on the kevy-server, blocking-reads published events,
/// and re-emits them into the local `bus` (skipping this process's own
/// publications). Reconnects with a fixed backoff if the connection
/// drops. The thread runs for the life of the process.
pub fn spawn_kevy_notify_bridge(url: String, channel: Vec<u8>, origin: String, bus: EventBus) {
    std::thread::Builder::new()
        .name("kevy-notify-bridge".into())
        .spawn(move || notify_bridge_loop(&url, &channel, &origin, &bus))
        .expect("spawn kevy-notify-bridge thread");
}

fn notify_bridge_loop(url: &str, channel: &[u8], origin: &str, bus: &EventBus) {
    loop {
        if let Ok(mut sub) = Subscriber::open(url, &[channel]) {
            // drain until the connection errors, then fall through to
            // reconnect.
            while let Ok((_chan, payload)) = sub.recv_message() {
                if let Ok(env) = serde_json::from_slice::<NotifyEnvelope>(&payload)
                    && env.origin != origin
                {
                    bus.emit_local(env.event);
                }
            }
        }
        // connection failed or dropped — back off before reconnecting.
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Publish from one EventBus, receive on another, both over a shared
    // named `mem://` bus (kevy's in-process pub/sub). Proves the
    // publish → subscribe → re-emit path and the origin self-skip guard
    // without a TCP server.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn publish_crosses_to_a_separate_subscriber_bus() {
        let url = "mem://notify-bridge-test";
        let channel = NOTIFY_CHANNEL.to_vec();

        // subscriber side: its own bus + origin, bridge running.
        let sub_bus = EventBus::new(64);
        let mut rx = sub_bus.subscribe();
        spawn_kevy_notify_bridge(
            url.to_string(),
            channel.clone(),
            "subscriber-origin".into(),
            sub_bus.clone(),
        );
        // give the bridge a moment to SUBSCRIBE before we publish.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // publisher side: a different origin.
        let client = Arc::new(KevyNetClient::new(url));
        let publisher = KevyEventPublisher::new(client, channel, "publisher-origin".into());
        publisher.publish(&SmtpEvent::NewMessage {
            user: "u".into(),
            thread_id: "t".into(),
            sender: "s".into(),
            subject: "hi".into(),
            snippet: "sn".into(),
        });

        // the subscriber bus should re-emit it locally.
        let got = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("bridge should deliver within timeout")
            .expect("recv ok");
        assert!(matches!(got.event, SmtpEvent::NewMessage { .. }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscriber_skips_its_own_origin() {
        let url = "mem://notify-skip-test";
        let channel = NOTIFY_CHANNEL.to_vec();
        let origin = "same-origin".to_string();

        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        spawn_kevy_notify_bridge(
            url.to_string(),
            channel.clone(),
            origin.clone(),
            bus.clone(),
        );
        tokio::time::sleep(Duration::from_millis(200)).await;

        // publish with the SAME origin the bridge runs under → skipped.
        let client = Arc::new(KevyNetClient::new(url));
        let publisher = KevyEventPublisher::new(client, channel, origin);
        publisher.publish(&SmtpEvent::InviteReceived {
            user: "u".into(),
            message_id: 1,
            method: "REQUEST".into(),
            uid: "x".into(),
        });

        // nothing should arrive (own-origin skipped).
        let res = tokio::time::timeout(Duration::from_millis(700), rx.recv()).await;
        assert!(res.is_err(), "own-origin event must not be re-emitted");
    }
}
