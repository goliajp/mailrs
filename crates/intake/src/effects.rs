//! What every arrival tells the rest of the system, on the network kevy
//! both cores share. A core calls [`before_store`] before it writes the
//! message and [`after_store`] once the message is readable.

use crate::arrival::Arrival;
use crate::{live_sync, push, webhook_delivery};

/// The senders join the recipient's contacts, and the message counts
/// against their usage on `net`, the core's network kevy, where
/// [`crate::mail_files`] takes it off again.
pub fn before_store(
    net: Option<kevy_client::Connection>,
    addr: &str,
    a: &Arrival<'_>,
    size: usize,
) {
    live_sync::upsert_contacts(addr, &a.from);
    let mut net = net;
    live_sync::adjust_usage_on(net.as_mut(), addr, size as i64);
}

/// Tell the clients last: a client that hears of new mail asks for its list
/// at once, and must find the message there. The webhook event is queued
/// here, once whether it was held is known.
///
/// `held` is whether this message's verdict held its conversation;
/// `in_review` whether the conversation is held for `addr` once the message
/// is stored, by this message or an earlier one. Mail in Review rings no
/// phone, and its new-mail event says it is held: the clients refresh what
/// they show, Review included, and raise no sound, notice or announcement.
/// A message arriving in a conversation held earlier has no verdict of its
/// own, so `held` alone let it ring.
pub fn after_store(
    mut net: Option<kevy_client::Connection>,
    addr: &str,
    a: &Arrival<'_>,
    held: bool,
    in_review: bool,
) {
    record_domains(net.as_mut(), a);
    record_callback_numbers(net.as_mut(), addr, a);
    let quiet = held || in_review;
    if !quiet {
        push::maybe_notify(net.as_mut(), addr, a);
    }
    webhook_delivery::enqueue_for_arrival(net, addr, a, held);
    live_sync::publish_new_mail(addr, &a.thread_id, &a.from, &a.subject, "", quiet);
}

/// The phone numbers in the message's text, into `addr`'s history with the
/// sender, at the arrival this deployment stamped on it: the reading pane
/// asks the history whether the sender gave a number before this message.
/// Not the reader's own mail. The attachments' numbers are added by the
/// core that reads them.
fn record_callback_numbers(net: Option<&mut kevy_client::Connection>, addr: &str, a: &Arrival<'_>) {
    let Some(conn) = net else { return };
    if a.is_own {
        return;
    }
    let numbers = mailrs_inbound::body::callback_numbers(&a.parsed);
    if numbers.is_empty() {
        return;
    }
    let at = mailrs_inbound::dates::received_epoch(a.head)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(e) = mailrs_core_sidestate::families::callback_numbers::record_numbers(
        conn,
        addr,
        &a.from,
        &numbers,
        at,
        &a.message_id,
    ) {
        tracing::warn!(error = %e, "callback numbers not recorded");
    }
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
