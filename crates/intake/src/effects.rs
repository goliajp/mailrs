//! What every arrival tells the rest of the system, on the network kevy
//! both cores share. A core calls [`before_store`] before it writes the
//! message and [`after_store`] once the message is readable.

use crate::arrival::Arrival;
use crate::{live_sync, push, webhook_delivery};

/// The senders join the recipient's contacts, and the message counts
/// against their usage.
pub fn before_store(addr: &str, a: &Arrival<'_>, size: usize) {
    live_sync::upsert_contacts(addr, &a.from);
    live_sync::adjust_usage_bytes(addr, size as i64);
}

/// Tell the clients last: a client that hears of new mail asks for its list
/// at once, and must find the message there. Mail held as fraud rings no
/// phone. The webhook event is queued here, once whether it was held is
/// known.
pub fn after_store(
    mut net: Option<kevy_client::Connection>,
    addr: &str,
    a: &Arrival<'_>,
    held: bool,
) {
    record_domains(net.as_mut(), a);
    if !held {
        push::maybe_notify(net.as_mut(), addr, a);
    }
    webhook_delivery::enqueue_for_arrival(net, addr, a, held);
    live_sync::publish_new_mail(addr, &a.thread_id, &a.from, &a.subject, "");
}

/// When the deployment first saw the sender's domain and its link domains;
/// times already recorded stay. Best-effort, like the other side effects:
/// a domain not recorded later reads as never seen.
fn record_domains(net: Option<&mut kevy_client::Connection>, a: &Arrival<'_>) {
    let Some(conn) = net else { return };
    let sender = mailrs_core_sidestate::families::correspondent::first_address(&a.from);
    let mut hosts: Vec<String> = sender
        .rsplit_once('@')
        .map(|(_, host)| vec![host.to_string()])
        .unwrap_or_default();
    hosts.extend(mailrs_inbound::scan_input::link_hosts(&a.parsed));
    let now = chrono::Utc::now().timestamp();
    if let Err(e) = mailrs_core_sidestate::families::domain_first::record(conn, &hosts, now) {
        tracing::warn!(error = %e, "first-seen domains not recorded");
    }
}
