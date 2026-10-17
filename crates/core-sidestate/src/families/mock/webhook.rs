//! A namespace's new-mail webhook: an ordinary subscription of the
//! namespace's owner (`event_type` [`EVENT`]), so signing, retries and the
//! delivery worker are the ones every webhook uses. The namespace hash
//! keeps its id and URL.

use kevy_client::Connection;
use mailrs_core_api::method::admin::WebhookSubWire;

use super::*;
use crate::families::{webhook_outbox, webhooks};

pub const EVENT: &str = "mock_message";

/// Point the namespace's webhook at `url`, replacing any earlier one.
/// Returns the subscription, signing secret included.
pub fn set_webhook(conn: &mut Connection, ns: &str, url: &str) -> MockResult<WebhookSubWire> {
    let Some(found) = get_namespace(conn, ns)? else {
        return Err(MockError::NotFound);
    };
    clear_webhook(conn, ns)?;
    let sub = webhooks::create(
        conn,
        webhooks::NewWebhook {
            account_address: found.owner,
            url: url.to_string(),
            event_type: EVENT.to_string(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )?;
    conn.hset(
        ns_key(ns).as_bytes(),
        &[
            (b"webhook_id".as_slice(), sub.id.to_string().as_bytes()),
            (b"webhook_url", url.as_bytes()),
        ],
    )?;
    Ok(sub)
}

/// Remove the namespace's webhook; `false` when it had none.
pub fn clear_webhook(conn: &mut Connection, ns: &str) -> MockResult<bool> {
    let Some(id) = webhook_id(conn, ns)? else {
        return Ok(false);
    };
    webhooks::delete(conn, id)?;
    conn.hdel(ns_key(ns).as_bytes(), &[b"webhook_id", b"webhook_url"])?;
    Ok(true)
}

fn webhook_id(conn: &mut Connection, ns: &str) -> MockResult<Option<i64>> {
    Ok(conn
        .hget(ns_key(ns).as_bytes(), b"webhook_id")?
        .and_then(|v| utf8(v).parse().ok()))
}

/// Queue the webhook delivery for a message just stored, when its
/// namespace has a webhook.
pub(super) fn notify(conn: &mut Connection, meta: &MessageMeta) -> MockResult<()> {
    let Some(id) = webhook_id(conn, &meta.namespace)? else {
        return Ok(());
    };
    let owner = conn
        .hget(ns_key(&meta.namespace).as_bytes(), b"owner")?
        .map(utf8)
        .unwrap_or_default();
    let payload = serde_json::json!({
        "event": EVENT,
        "timestamp": meta.received_at / 1000,
        "data": meta,
    });
    webhook_outbox::enqueue(
        conn,
        id,
        &owner,
        &payload.to_string(),
        meta.received_at / 1000,
    )?;
    Ok(())
}
