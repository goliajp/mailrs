//! UnifiedPush: "you have mail" to the reader's Android phones, through the
//! distributor each phone chose. The push policy has already said yes.
//!
//! The push says nothing about the message: no sender, no subject, no
//! count. The distributor's server is somebody else's, so all it carries is
//! that the phone should look; the app then asks this server what arrived,
//! over its own signed-in connection. The request is a Web Push (RFC 8030,
//! encrypted per RFC 8291, VAPID-signed with the Web Push key), which is
//! what UnifiedPush distributors take, so the channel is on exactly when
//! Web Push is.

use std::sync::Arc;

use mailrs_core_sidestate::families::push_devices;
use mailrs_core_sidestate::families::unified_push::{self, Device};
use mailrs_webpush::send::{self, Outcome, Subscription};
use mailrs_webpush::vapid::Vapid;

/// The whole message. Not empty: an empty push reaches the app as no
/// message at all.
pub const PING: &[u8] = b"new-mail";

/// The counter channel.
pub const CHANNEL: &str = push_devices::Channel::UnifiedPush.as_str();

/// A stored device in the form the sender takes; `None` when its keys do
/// not decode.
fn subscription(d: &Device) -> Option<Subscription> {
    Some(Subscription {
        endpoint: d.endpoint.clone(),
        p256dh: mailrs_webpush::from_b64url(&d.p256dh)?,
        auth: mailrs_webpush::from_b64url(&d.auth)?,
    })
}

/// Push [`PING`] to each device with `push`, one after another; each
/// device's id and what became of its push. A device whose keys do not
/// decode is gone: nothing could ever be sent to it.
pub async fn round<F, Fut>(devices: Vec<(String, Device)>, push: F) -> Vec<(String, Outcome)>
where
    F: Fn(Subscription) -> Fut,
    Fut: std::future::Future<Output = Outcome>,
{
    let mut out = Vec::with_capacity(devices.len());
    for (id, device) in devices {
        let outcome = match subscription(&device) {
            Some(sub) => push(sub).await,
            None => Outcome::Gone,
        };
        out.push((id, outcome));
    }
    out
}

/// Count each outcome and forget the devices whose distributor no longer
/// knows them (404 or 410).
pub fn settle(
    conn: &mut kevy_client::Connection,
    user: &str,
    outcomes: &[(String, Outcome)],
    now: i64,
) -> std::io::Result<()> {
    use mailrs_core_sidestate::families::notify;
    for (id, outcome) in outcomes {
        let counted = match outcome {
            Outcome::Sent => "sent",
            _ => "failed",
        };
        notify::count(conn, user, CHANNEL, counted, now)?;
        match outcome {
            Outcome::Sent => {
                tracing::info!(%user, "unifiedpush: delivered to the distributor");
                push_devices::mark_used(conn, user, push_devices::Channel::UnifiedPush, id, now)?;
            }
            Outcome::Gone => {
                tracing::info!(%user, "unifiedpush: dropping a device its distributor no longer knows");
                unified_push::remove(conn, user, id)?;
            }
            Outcome::Failed(reason) => tracing::warn!(%user, %reason, "unifiedpush: push failed"),
        }
    }
    Ok(())
}

/// Ping every phone `addr` registered, in the background, at most once per
/// [`coalesce::WINDOW`] (see there for why the last mail of a burst is not
/// lost).
pub fn spawn(vapid: Arc<Vapid>, addr: &str, now: i64) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(%addr, "unifiedpush: no runtime handle, no push");
        return;
    };
    let user = addr.to_string();
    handle.spawn(async move {
        match coalesce::gate(connect, &user, coalesce::WINDOW).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(%user, "unifiedpush: a ping still to come covers this message");
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, %user, "unifiedpush: ping window unreadable, no push");
                return;
            }
        }
        let devices = match tokio::task::spawn_blocking({
            let user = user.clone();
            move || devices(&mut connect()?, &user)
        })
        .await
        {
            Ok(Ok(devices)) => devices,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, %user, "unifiedpush: device read failed");
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "unifiedpush: device read join failed");
                return;
            }
        };
        if devices.is_empty() {
            return;
        }
        let outcomes = round(devices, |sub| {
            let vapid = vapid.clone();
            async move { send::send_to_distributor(&vapid, &sub, PING, now).await }
        })
        .await;
        let settled =
            tokio::task::spawn_blocking(move || settle(&mut connect()?, &user, &outcomes, now))
                .await;
        match settled {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "unifiedpush: outcomes not recorded"),
            Err(e) => tracing::warn!(error = %e, "unifiedpush: outcome join failed"),
        }
    });
}

/// The phones `user` has to be pinged now.
fn devices(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<Vec<(String, Device)>> {
    crate::push::drop_signed_out(conn, user, push_devices::Channel::UnifiedPush)?;
    unified_push::list(conn, user)
}

fn connect() -> std::io::Result<kevy_client::Connection> {
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    Ok(kevy_client::Connection::connect(&url)?)
}

pub mod coalesce;

#[cfg(test)]
mod tests;
