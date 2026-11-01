//! One push: encrypted, signed, and sent to the address that was checked.

use std::time::Duration;

use crate::ece::{self, Receiver};
use crate::endpoint;
use crate::vapid::Vapid;

/// A browser's subscription, as it hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's `p256dh` key, decoded.
    pub p256dh: Vec<u8>,
    /// The browser's `auth` secret, decoded.
    pub auth: Vec<u8>,
}

/// What became of one push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    /// The push service no longer knows the subscription: drop it.
    Gone,
    /// Refused before sending, or failed; the reason, shortened.
    Failed(String),
}

/// How long the push service keeps an undelivered message.
const TTL_SECS: u32 = 24 * 3600;

/// Push `payload` to `sub`.
pub async fn send(vapid: &Vapid, sub: &Subscription, payload: &[u8], now: i64) -> Outcome {
    let Ok(url) = url::Url::parse(&sub.endpoint) else {
        return Outcome::Failed("not a URL".into());
    };
    let (host, addr) = match endpoint::resolve(&url).await {
        Ok(v) => v,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let body = match ece::encrypt(
        payload,
        &Receiver {
            public: &sub.p256dh,
            auth: &sub.auth,
        },
    ) {
        Ok(b) => b,
        Err(e) => return Outcome::Failed(format!("{e:?}")),
    };
    let Ok(auth) = vapid.authorization(&url, now) else {
        return Outcome::Failed("signing failed".into());
    };
    // connect to the address checked above, not a second lookup's answer
    let client = match reqwest::Client::builder()
        .resolve(&host, addr)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let res = client
        .post(url)
        .header("TTL", TTL_SECS.to_string())
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header("Urgency", "normal")
        .header("Authorization", auth)
        .body(body)
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => Outcome::Sent,
        Ok(r) if matches!(r.status().as_u16(), 404 | 410) => Outcome::Gone,
        Ok(r) => Outcome::Failed(format!("push service answered {}", r.status().as_u16())),
        Err(e) => Outcome::Failed(e.to_string().chars().take(200).collect()),
    }
}
