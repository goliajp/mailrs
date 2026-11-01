//! One webhook event, matched against its user's subscriptions and queued
//! for each that admits it — the one routine every writer calls, whichever
//! process it runs in.
//!
//! The payload carries metadata only: ids, addresses, a subject. No body,
//! no attachment, nothing that signs anyone in.

use kevy_client::Connection;
/// The event types, for writers that do not depend on core-api.
pub use mailrs_core_api::webhook_events as kinds;
use mailrs_core_api::webhook_events::subscribed_to;

use super::{webhook_outbox, webhooks};

/// The longest remote reply text a payload carries.
const REASON_CHARS: usize = 500;

/// What happened, to whom.
#[derive(Debug, Clone)]
pub struct Event<'a> {
    /// One of [`mailrs_core_api::webhook_events::ALL`].
    pub kind: &'static str,
    pub user: &'a str,
    /// For sender filters; `None` on events without a sender.
    pub sender: Option<&'a str>,
    /// For thread filters; `None` on events without a conversation.
    pub thread_id: Option<&'a str>,
    /// The event's own fields; `user` (and `thread_id`, `sender` when given)
    /// are added.
    pub data: serde_json::Value,
}

/// A remote server's reply text, cut to what a payload carries.
pub fn reason(text: &str) -> String {
    text.chars().take(REASON_CHARS).collect()
}

/// The body POSTed for `event` to a subscription stored as `subscribed`:
/// `event` names what the subscription asked for, so a subscription under
/// the old name `new_message` keeps reading `new_message`.
pub fn payload(subscribed: &str, event: &Event<'_>, timestamp: &str) -> serde_json::Value {
    let mut data = match &event.data {
        serde_json::Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    data.insert("user".into(), event.user.into());
    if let Some(t) = event.thread_id {
        data.insert("thread_id".into(), t.into());
    }
    if let Some(s) = event.sender {
        data.insert("sender".into(), s.into());
    }
    serde_json::json!({
        "event": subscribed,
        "version": 1,
        "timestamp": timestamp,
        "data": data,
    })
}

/// Queue `event` for every subscription of its user that admits it; how
/// many were queued.
pub fn enqueue(conn: &mut Connection, event: &Event<'_>, now: i64) -> std::io::Result<usize> {
    let subs: Vec<_> = webhooks::list(conn, event.user)?
        .into_iter()
        .filter(|s| {
            subscribed_to(&s.event_type, event.kind)
                && webhooks::matches(s, event.sender, event.thread_id)
        })
        .collect();
    if subs.is_empty() {
        return Ok(0);
    }
    let timestamp = chrono::DateTime::from_timestamp(now, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    for sub in &subs {
        let body = payload(&sub.event_type, event, &timestamp).to_string();
        webhook_outbox::enqueue(conn, sub.id, event.user, &body, now)?;
    }
    Ok(subs.len())
}

/// [`enqueue`] where a failure must not stop the caller: logged, not
/// returned. A webhook that cannot be queued is not a reason to fail the
/// delivery, the send or the click that raised it.
pub fn enqueue_logged(conn: &mut Connection, event: &Event<'_>, now: i64) {
    if let Err(e) = enqueue(conn, event, now) {
        tracing::warn!(err = %e, kind = event.kind, user = event.user, "webhook: could not queue");
    }
}

/// Queue the thread event a core call raised: the new state under its
/// field, and the label for `thread.labelled`. Logged, not returned.
pub fn thread_changed(
    conn: &mut Connection,
    user: &str,
    thread_id: &str,
    label_id: Option<&str>,
    change: kinds::ThreadChange,
    now: i64,
) {
    let mut data = serde_json::json!({ change.field: change.on });
    if let Some(l) = label_id {
        data["label_id"] = l.into();
    }
    let event = Event {
        kind: change.event,
        user,
        sender: None,
        thread_id: Some(thread_id),
        data,
    };
    enqueue_logged(conn, &event, now);
}

#[cfg(test)]
mod tests;
