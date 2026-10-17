use kevy_client::Connection;

use super::*;

/// Counters for `/metrics`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub received: i64,
    pub rejected_unknown_ns: i64,
    pub bytes: i64,
    pub dropped: i64,
}

pub fn stats(conn: &mut Connection) -> MockResult<Stats> {
    let int = |c: &mut Connection, k: &[u8]| -> MockResult<i64> {
        Ok(c.get(k)?.and_then(|v| utf8(v).parse().ok()).unwrap_or(0))
    };
    let mut dropped = 0;
    for ns in conn.smembers(NS_INDEX)? {
        dropped += int(conn, dropped_key(&utf8(ns)).as_bytes())?;
    }
    Ok(Stats {
        received: int(conn, RECEIVED)?,
        rejected_unknown_ns: int(conn, REJECTED_UNKNOWN_NS)?,
        bytes: int(conn, BYTES)?,
        dropped,
    })
}

/// Count an SMTP transaction refused because it named no namespace.
pub fn count_rejected_unknown_ns(conn: &mut Connection) -> MockResult<()> {
    conn.incr(REJECTED_UNKNOWN_NS)?;
    Ok(())
}
