//! Receiver-facing connection-metrics port.

/// Connection + inbound-verdict counters the receiving path records.
/// Abstracted so the receiver records metrics without holding the whole
/// spg/kevy-bound `WebState`.
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
