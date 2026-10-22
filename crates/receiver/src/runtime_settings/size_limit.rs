//! The largest message the receiver takes, as set on the System Config
//! page: field `max_message_size_bytes` of the network-kevy hash
//! `admin:system-config`, a decimal byte count written by webapi.
//!
//! Absent means the compile-time `MAX_MESSAGE_SIZE`, which is also the
//! ceiling. The codec holds a whole DATA payload in memory until the
//! terminator arrives, and the receiver's memory limit in compose is
//! sized against that constant, so the page can lower the limit but not
//! raise it; a larger stored value is held to the constant.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mailrs_smtp_proto::MAX_MESSAGE_SIZE;

use crate::kevy_net::KevyNetClient;

pub use mailrs_core_sidestate::keys::admin::SYSTEM_CONFIG as CONFIG_KEY;
pub const MAX_MESSAGE_SIZE_FIELD: &[u8] = b"max_message_size_bytes";

const TTL: Duration = Duration::from_secs(60);

pub struct MessageSizeLimit {
    client: Option<Arc<KevyNetClient>>,
    cached: Mutex<Option<(Instant, u64)>>,
}

impl MessageSizeLimit {
    /// `None` never reads a store: always `MAX_MESSAGE_SIZE`.
    pub fn new(client: Option<Arc<KevyNetClient>>) -> Self {
        Self {
            client,
            cached: Mutex::new(None),
        }
    }

    /// The limit for a connection starting now. The store is read at most
    /// once a minute; a failed read keeps the last value for that minute.
    pub async fn current(&self) -> u64 {
        let Some(client) = &self.client else {
            return MAX_MESSAGE_SIZE;
        };
        let last = *self.cached.lock().expect("size limit lock");
        if let Some((at, value)) = last
            && at.elapsed() < TTL
        {
            return value;
        }
        let c = client.clone();
        let read = tokio::task::spawn_blocking(move || {
            c.with_conn(|conn| {
                conn.hget(CONFIG_KEY, MAX_MESSAGE_SIZE_FIELD)
                    .map_err(std::io::Error::from)
            })
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        let value = match read {
            Ok(raw) => parse(raw.as_deref()),
            Err(e) => {
                tracing::warn!(error = %e, "could not read the message size limit; keeping the last one");
                last.map_or(MAX_MESSAGE_SIZE, |(_, v)| v)
            }
        };
        *self.cached.lock().expect("size limit lock") = Some((Instant::now(), value));
        value
    }
}

fn parse(raw: Option<&[u8]>) -> u64 {
    let Some(raw) = raw else {
        return MAX_MESSAGE_SIZE;
    };
    let parsed = std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    match parsed {
        Some(n) if n > 0 => n.min(MAX_MESSAGE_SIZE),
        _ => {
            tracing::warn!(
                value = %String::from_utf8_lossy(raw),
                "stored max_message_size_bytes is not a positive byte count; using the built-in limit"
            );
            MAX_MESSAGE_SIZE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_MESSAGE_SIZE, parse};

    #[test]
    fn absent_or_unusable_is_the_built_in_limit() {
        assert_eq!(parse(None), MAX_MESSAGE_SIZE);
        assert_eq!(parse(Some(b"lots")), MAX_MESSAGE_SIZE);
        // SIZE 0 would advertise "no limit" (RFC 1870)
        assert_eq!(parse(Some(b"0")), MAX_MESSAGE_SIZE);
    }

    #[test]
    fn the_page_can_lower_the_limit_but_not_raise_it() {
        assert_eq!(parse(Some(b"1048576")), 1_048_576);
        assert_eq!(parse(Some(b"99999999999")), MAX_MESSAGE_SIZE);
    }
}
