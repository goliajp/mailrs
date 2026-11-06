//! Web Push: the browser side of "you have mail". The push policy has
//! already said yes; this builds the message, sends it to every browser
//! the reader subscribed, drops subscriptions the push service no longer
//! knows, and counts each outcome under the `web` channel.

use std::sync::{Arc, OnceLock};

use mailrs_core_sidestate::families::notify;
use mailrs_core_sidestate::families::push_devices::Channel;
use mailrs_webpush::send::{self, Outcome, Subscription};
use mailrs_webpush::vapid::Vapid;

/// `Some` when a VAPID key is configured and readable; resolved once.
pub fn vapid() -> Option<Arc<Vapid>> {
    static VAPID: OnceLock<Option<Arc<Vapid>>> = OnceLock::new();
    VAPID
        .get_or_init(|| match Vapid::from_env() {
            Ok(v) => v.map(Arc::new),
            Err(e) => {
                tracing::warn!(error = %e, "web push: key not usable — disabled");
                None
            }
        })
        .clone()
}

/// What the service worker gets. With the content hidden it carries only
/// what opening the conversation needs.
pub fn payload(thread_id: &str, sender: &str, subject: &str, hide_content: bool) -> Vec<u8> {
    let id = notification_id();
    let body = match hide_content {
        true => serde_json::json!({
            "thread_id": thread_id,
            "notification_id": id,
            "hidden": true,
        }),
        false => serde_json::json!({
            "thread_id": thread_id,
            "notification_id": id,
            // header values arrive attacker-sized; push services cap at 4KB
            "sender": crate::push::truncated(sender, 200),
            "subject": crate::push::truncated(subject, 300),
        }),
    };
    body.to_string().into_bytes()
}

fn notification_id() -> String {
    use rand_core::RngCore;
    format!("{:016x}", rand_core::OsRng.next_u64())
}

/// A stored subscription in the form the sender takes; `None` when its
/// keys do not decode.
fn subscription(s: &notify::WebSubscription) -> Option<Subscription> {
    Some(Subscription {
        endpoint: s.endpoint.clone(),
        p256dh: mailrs_webpush::from_b64url(&s.p256dh)?,
        auth: mailrs_webpush::from_b64url(&s.auth)?,
    })
}

/// Send `payload` to every browser `addr` subscribed, in the background.
pub fn spawn(vapid: Arc<Vapid>, addr: &str, payload: Vec<u8>, now: i64) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(%addr, "web push: no runtime handle, no push");
        return;
    };
    let user = addr.to_string();
    handle.spawn(async move {
        let subs = match tokio::task::spawn_blocking({
            let user = user.clone();
            move || load(&user)
        })
        .await
        {
            Ok(Ok(subs)) => subs,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, %user, "web push: subscription read failed");
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "web push: subscription read join failed");
                return;
            }
        };
        for stored in subs {
            let outcome = match subscription(&stored) {
                Some(sub) => send::send(&vapid, &sub, &payload, now).await,
                None => Outcome::Gone,
            };
            let counted = match outcome {
                Outcome::Sent => "sent",
                _ => "failed",
            };
            let who = user.clone();
            let _ = tokio::task::spawn_blocking(move || {
                crate::push::count(&who, "web", counted, now)
            })
            .await;
            match outcome {
                Outcome::Sent => {
                    tracing::info!(%user, "web push: delivered to the push service");
                    let who = user.clone();
                    let id = notify::endpoint_field(&stored.endpoint);
                    let _ = tokio::task::spawn_blocking(move || {
                        crate::push::mark_used(&who, Channel::Web, &id, now)
                    })
                    .await;
                }
                Outcome::Gone => {
                    tracing::info!(%user, "web push: dropping a subscription the service no longer knows");
                    let who = user.clone();
                    let _ = tokio::task::spawn_blocking(move || forget(&who, &stored.endpoint)).await;
                }
                Outcome::Failed(reason) => {
                    tracing::warn!(%user, %reason, "web push: push failed")
                }
            }
        }
    });
}

fn connect() -> std::io::Result<kevy_client::Connection> {
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    Ok(kevy_client::Connection::connect(&url)?)
}

fn load(user: &str) -> std::io::Result<Vec<notify::WebSubscription>> {
    subscriptions(&mut connect()?, user)
}

/// The browsers `user` has to be pushed to now.
fn subscriptions(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<Vec<notify::WebSubscription>> {
    crate::push::drop_signed_out(conn, user, Channel::Web)?;
    notify::web_subscriptions(conn, user)
}

fn forget(user: &str, endpoint: &str) {
    if let Ok(mut conn) = connect() {
        let _ = notify::remove_web_subscription(&mut conn, user, endpoint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_content_leaves_sender_and_subject_out() {
        let shown: serde_json::Value =
            serde_json::from_slice(&payload("t1", "Boss <b@x>", "Raise", false)).unwrap();
        assert_eq!(shown["thread_id"], "t1");
        assert_eq!(shown["sender"], "Boss <b@x>");
        assert_eq!(shown["subject"], "Raise");
        assert!(
            shown["notification_id"]
                .as_str()
                .is_some_and(|s| s.len() == 16)
        );

        let hidden: serde_json::Value =
            serde_json::from_slice(&payload("t1", "Boss <b@x>", "Raise", true)).unwrap();
        assert_eq!(hidden["thread_id"], "t1");
        assert_eq!(hidden["hidden"], true);
        let text = String::from_utf8(payload("t1", "Boss <b@x>", "Raise", true)).unwrap();
        assert!(!text.contains("Boss") && !text.contains("Raise"));
    }

    #[test]
    fn web_push_skips_and_drops_a_browser_whose_session_expired() {
        use crate::push::tests::{by, expire, session};
        use mailrs_core_sidestate::families::push_devices::{self, Registration};
        let mut c = kevy_client::Connection::connect("mem://web-push-signed-out").unwrap();
        let (old, live) = (session(&mut c, "old"), session(&mut c, "live"));
        let browser = |n: u32| notify::WebSubscription {
            endpoint: format!("https://fcm.googleapis.com/fcm/send/{n}"),
            p256dh: "k".into(),
            auth: "a".into(),
            created_at: i64::from(n),
        };
        let subs = [browser(1), browser(2), browser(3)];
        for (sub, s) in subs.iter().zip([Some(&old), Some(&live), None]) {
            let who = by(s.map(String::as_str));
            push_devices::register(&mut c, "me@x.test", Registration::Web(sub), who, 1).unwrap();
        }
        expire(&mut c, "old");
        let mut pushed: Vec<String> = subscriptions(&mut c, "me@x.test")
            .unwrap()
            .into_iter()
            .map(|s| s.endpoint)
            .collect();
        pushed.sort();
        assert_eq!(
            pushed,
            vec![subs[1].endpoint.clone(), subs[2].endpoint.clone()]
        );
        assert_eq!(
            notify::web_subscriptions(&mut c, "me@x.test")
                .unwrap()
                .len(),
            2
        );
    }
}
