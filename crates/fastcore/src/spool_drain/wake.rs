//! Wake the drain as soon as the receiver has spooled a message.
//!
//! The receiver publishes `SpoolDelivered` on [`NOTIFY_CHANNEL`] after the
//! file is in `incoming/new/`. Without a reader, new mail waited for the
//! next tick of the poll, up to its whole interval. The interval stays as
//! the floor for when this subscription is down.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core::event_bus::NOTIFY_CHANNEL;
use tokio::sync::Notify;

pub(super) fn spawn(kevy_url: String, wake: Arc<Notify>) {
    const RECONNECT: Duration = Duration::from_secs(1);
    std::thread::Builder::new()
        .name("spool-wake".into())
        .spawn(move || {
            loop {
                match kevy_client::Subscriber::connect_channels(&kevy_url, &[NOTIFY_CHANNEL]) {
                    Ok(mut sub) => {
                        tracing::info!("spool drain wakes on spooled mail");
                        while let Ok((_, payload)) = sub.recv_message() {
                            if is_spool_delivered(&payload) {
                                wake.notify_one();
                            }
                        }
                    }
                    Err(e) => tracing::warn!(err = %e, "spool wake subscribe failed; retry 1s"),
                }
                std::thread::sleep(RECONNECT);
            }
        })
        .expect("spawn spool-wake thread");
}

/// Whether a frame on the channel says a message was spooled. The frame
/// comes from another process, so anything else, or anything malformed,
/// is not.
fn is_spool_delivered(payload: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("event")?
                .get("type")?
                .as_str()
                .map(|t| t == "SpoolDelivered")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_spool_delivered;

    #[test]
    fn only_a_spooled_message_wakes_the_drain() {
        let spooled = serde_json::json!({
            "origin": "1-2",
            "event": mailrs_core::event_bus::SmtpEvent::SpoolDelivered {
                spool_id: "f".into(),
                recipient_count: 1,
            },
        });
        assert!(is_spool_delivered(spooled.to_string().as_bytes()));
        assert!(!is_spool_delivered(
            br#"{"origin":"1-2","event":{"type":"NewMessage"}}"#
        ));
        assert!(!is_spool_delivered(b"not json"));
    }
}
