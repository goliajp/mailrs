//! Mail submitted over SMTP from a masked address. Its `Received:` header
//! names neither the client's IP nor its EHLO name; this log keeps them
//! instead, for abuse handling: who signed in, which mask (its first eight
//! characters), from which IP, when.
//!
//! Kept apart from the admin audit log and capped on its own, so a mask
//! that sends a lot cannot push administrators' actions out of theirs.
//! Newest first.

use std::io;

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

use super::SUBMIT_LOG;

/// Rows kept.
pub const CAP: i64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    /// Unix seconds.
    pub at: i64,
    /// The account signed in.
    pub account: String,
    /// The mask's first eight characters.
    pub mask: String,
    /// The client's IP.
    pub ip: String,
}

/// Append one row, trimming to [`CAP`] once the list is a tenth past it.
pub fn append(c: &mut Connection, row: &Submission) -> io::Result<()> {
    let payload = serde_json::to_vec(row).map_err(io::Error::other)?;
    c.lpush(SUBMIT_LOG, &[payload.as_slice()])?;
    // kevy-client has no LTRIM: rewrite to the newest CAP rows, once per
    // CAP/10 appends
    if c.llen(SUBMIT_LOG)? as i64 > CAP + CAP / 10 {
        let keep = c.lrange(SUBMIT_LOG, 0, CAP - 1)?;
        c.del(&[SUBMIT_LOG])?;
        for row in keep.iter().rev() {
            c.lpush(SUBMIT_LOG, &[row.as_slice()])?;
        }
    }
    Ok(())
}

/// The newest `limit` rows.
pub fn recent(c: &mut Connection, limit: i64) -> io::Result<Vec<Submission>> {
    let rows = c.lrange(SUBMIT_LOG, 0, limit - 1)?;
    Ok(rows
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::admin::AUDIT_LOG;

    #[test]
    fn rows_come_back_newest_first_and_stay_out_of_the_audit_log() {
        let mut c = Connection::connect("mem://masked-submit-log").unwrap();
        for (at, ip) in [(1, "192.0.2.1"), (2, "192.0.2.2")] {
            let row = Submission {
                at,
                account: "me@x.jp".into(),
                mask: "amber.ot".into(),
                ip: ip.into(),
            };
            append(&mut c, &row).unwrap();
        }
        let got = recent(&mut c, 10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].ip, "192.0.2.2");
        assert_eq!(recent(&mut c, 1).unwrap().len(), 1);
        assert_eq!(c.llen(AUDIT_LOG).unwrap(), 0);
    }
}
