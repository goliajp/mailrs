//! What every arrival tells the rest of the system, on the network kevy
//! both cores share. A core calls [`before_store`] before it writes the
//! message and [`after_store`] once the message is readable.

use crate::arrival::Arrival;
use crate::{live_sync, push, webhook_delivery};

/// The subscriptions that admit this message are queued, the senders join
/// the recipient's contacts, and the message counts against their usage.
pub fn before_store(
    net: Option<kevy_client::Connection>,
    addr: &str,
    a: &Arrival<'_>,
    size: usize,
) {
    webhook_delivery::enqueue_for_arrival(net, addr, &a.thread_id, &a.from, &a.subject);
    live_sync::upsert_contacts(addr, &a.from);
    live_sync::adjust_usage_bytes(addr, size as i64);
}

/// Tell the clients last: a client that hears of new mail asks for its list
/// at once, and must find the message there. Mail held as fraud rings no
/// phone.
pub fn after_store(addr: &str, a: &Arrival<'_>, held: bool) {
    if !held {
        push::maybe_notify(addr, &a.from, &a.subject, a.category, a.is_own);
    }
    live_sync::publish_new_mail(addr, &a.thread_id, &a.from, &a.subject, "");
}
