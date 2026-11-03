//! Notices to an account's owner about a change made somewhere webapi does
//! not see, such as a Sieve script stored over ManageSieve in a core.
//! The core queues one; webapi takes it and mails the owner the way it
//! mails about a change made on the web.
//!
//! ```text
//! owner-notices   list of JSON OwnerNotice, LPUSH by the core, RPOP by webapi
//! ```

use serde::{Deserialize, Serialize};

/// The queue.
pub const KEY: &str = "owner-notices";

/// What changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A Sieve script that redirects mail to these addresses was stored.
    SieveRedirects { targets: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerNotice {
    pub user: String,
    pub change: Change,
    /// Where the change was made (`managesieve`).
    pub via: String,
    pub at: i64,
}

/// Queue `notice`.
///
/// # Errors
/// The store could not be written.
pub fn push(conn: &mut kevy_client::Connection, notice: &OwnerNotice) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(notice).map_err(std::io::Error::other)?;
    conn.lpush(KEY.as_bytes(), &[bytes.as_slice()])?;
    Ok(())
}

/// Take up to `max` notices, oldest first. One that does not decode is
/// dropped with a warning: it can never be sent.
///
/// # Errors
/// The store could not be read.
pub fn take(conn: &mut kevy_client::Connection, max: usize) -> std::io::Result<Vec<OwnerNotice>> {
    let raw = conn.rpop(KEY.as_bytes(), max)?;
    Ok(raw
        .into_iter()
        .filter_map(|b| match serde_json::from_slice(&b) {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!(error = %e, "owner notice does not decode; dropped");
                None
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_come_out_in_the_order_they_went_in() {
        let mut c = kevy_client::Connection::connect("mem://owner-notices-order").unwrap();
        for (i, t) in ["a@out.test", "b@out.test"].iter().enumerate() {
            let n = OwnerNotice {
                user: "u@x.test".into(),
                change: Change::SieveRedirects {
                    targets: vec![t.to_string()],
                },
                via: "managesieve".into(),
                at: i as i64,
            };
            push(&mut c, &n).unwrap();
        }
        let got = take(&mut c, 10).unwrap();
        assert_eq!(got.iter().map(|n| n.at).collect::<Vec<_>>(), [0, 1]);
        assert!(take(&mut c, 10).unwrap().is_empty());
    }
}
