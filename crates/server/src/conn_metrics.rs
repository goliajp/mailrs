//! Receiver-facing connection-metrics seam.
//!
//! The SMTP receiving path bumps a handful of counters/gauges as
//! connections open and close and as the inbound verdict is decided. Those
//! are abstracted behind [`ConnectionMetrics`] so the receiver doesn't hold
//! the whole [`WebState`] (which binds spg / kevy / mailbox / domain) just
//! to record metrics — in-process today, a lightweight sink in the
//! receiver-split topology.

use crate::web::WebState;

/// Connection + inbound-verdict counters the receiving path records.
pub trait ConnectionMetrics: Send + Sync {
    /// A new connection was accepted.
    fn on_connect(&self);
    /// A connection closed.
    fn on_disconnect(&self);
    /// A message was delivered to one or more local recipients.
    fn on_message_delivered(&self);
    /// The inbound pipeline accepted a message.
    fn inbound_accept(&self);
    /// The inbound pipeline rejected a message (spam / virus / DMARC).
    fn inbound_reject(&self);
    /// The inbound pipeline deferred a message (greylist).
    fn inbound_defer(&self);
    /// The inbound pipeline routed a message to Junk.
    fn inbound_junk(&self);
}

impl ConnectionMetrics for WebState {
    fn on_connect(&self) {
        WebState::on_connect(self);
    }
    fn on_disconnect(&self) {
        WebState::on_disconnect(self);
    }
    fn on_message_delivered(&self) {
        WebState::on_message_delivered(self);
    }
    fn inbound_accept(&self) {
        self.inbound_accept_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn inbound_reject(&self) {
        self.inbound_reject_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn inbound_defer(&self) {
        self.inbound_defer_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn inbound_junk(&self) {
        self.inbound_junk_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
