//! Where a sender host's web root lands.
//!
//! ```text
//! mailrs:sitelanding:{host}   string   `at:<final host>` or `nowhere`;
//!                                      expires after 30 days (1 day for
//!                                      `nowhere`)
//! ```
//!
//! Written by the receive path after it visits `http://{host}/` for a
//! message worth the visit ([`mailrs_fraud::burner_domain::worth_visiting`]),
//! read by every judging of stored mail, which never visits anything
//! itself. A cache of the outside world, so it expires: a domain can be
//! sold, and its site moved.

use std::io;
use std::time::Duration;

use kevy_client::Connection;
use mailrs_fraud::burner_domain::{Landing, ttl_seconds};

const PREFIX: &str = "mailrs:sitelanding:";

/// The key for `host`'s landing.
pub fn key(host: &str) -> String {
    format!(
        "{PREFIX}{}",
        host.trim().trim_end_matches('.').to_ascii_lowercase()
    )
}

/// The landing recorded for `host`, if one is still kept.
pub fn get(conn: &mut Connection, host: &str) -> io::Result<Option<Landing>> {
    let value = conn.get(key(host).as_bytes())?;
    Ok(value.and_then(|v| Landing::decode(&String::from_utf8_lossy(&v))))
}

/// Record where `host`'s web root landed.
pub fn put(conn: &mut Connection, host: &str, landing: &Landing) -> io::Result<()> {
    conn.set_with_ttl(
        key(host).as_bytes(),
        landing.encode().as_bytes(),
        Duration::from_secs(ttl_seconds(landing)),
    )?;
    Ok(())
}
