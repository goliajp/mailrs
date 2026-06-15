//! In-process adapter for the receiver's [`ConnectionMetrics`] port: the
//! [`WebState`] implementation. The port (trait) lives in `mailrs-receiver`;
//! this is the core-side adapter that records into WebState's counters /
//! gauges.

use mailrs_receiver::ConnectionMetrics;

use crate::web::WebState;

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
