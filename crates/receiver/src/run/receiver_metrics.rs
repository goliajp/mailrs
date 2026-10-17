use crate::conn_metrics::ConnectionMetrics;
use crate::stats::SharedCounters;

/// Connection / inbound-verdict metrics for the receiver. Emits prometheus
/// counters via the global `metrics` recorder (a no-op if none is installed),
/// and keeps the same three numbers in process-local atomics — prometheus is
/// a scrape endpoint, and the live monitor wants the current values pushed
/// to it over a socket.
pub(super) struct ReceiverMetrics {
    pub(super) counters: SharedCounters,
}

impl ConnectionMetrics for ReceiverMetrics {
    fn on_connect(&self) {
        self.counters.on_connect();
        metrics::counter!("mailrs_receiver_connections_total").increment(1);
    }
    fn on_disconnect(&self) {
        self.counters.on_disconnect();
        metrics::counter!("mailrs_receiver_disconnects_total").increment(1);
    }
    fn on_message_delivered(&self) {
        self.counters.on_message();
        metrics::counter!("mailrs_receiver_spooled_total").increment(1);
    }
    fn inbound_accept(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "accept").increment(1);
    }
    fn inbound_reject(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "reject").increment(1);
    }
    fn inbound_defer(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "defer").increment(1);
    }
    fn inbound_junk(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "junk").increment(1);
    }
}
