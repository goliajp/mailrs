//! Accounts whose mail is also delivered to other accounts here.
//!
//! ```text
//! mailrs:forward:local:{address}   set of target account addresses
//! ```
//!
//! Written by the webapi's admin route, read by the spool drain of either
//! core: after a message lands in `address`'s own mailbox, a copy is
//! written straight into each target's mailbox, under the verdict the
//! message already has. Nothing is queued or sent over SMTP, and a copy
//! delivered this way is not forwarded again.

use kevy_client::Connection;

const PREFIX: &str = "mailrs:forward:local:";

/// Key of `address`'s forward targets.
#[must_use]
pub fn key(address: &str) -> String {
    format!("{PREFIX}{}", address.trim().to_ascii_lowercase())
}

/// `address`'s forward targets, lowercased and sorted.
///
/// # Errors
/// The store could not be read — distinct from no targets.
pub fn targets(conn: &mut Connection, address: &str) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = conn
        .smembers(key(address).as_bytes())?
        .iter()
        .filter_map(|m| std::str::from_utf8(m).ok())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Replace `address`'s targets with `targets`; an empty list removes them.
///
/// # Errors
/// The store could not be written.
pub fn set_targets(
    conn: &mut Connection,
    address: &str,
    targets: &[String],
) -> std::io::Result<()> {
    let k = key(address);
    conn.del(&[k.as_bytes()])?;
    if targets.is_empty() {
        return Ok(());
    }
    let members: Vec<String> = targets
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .collect();
    let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
    conn.sadd(k.as_bytes(), &refs)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_replaced_and_cleared() {
        let mut conn = Connection::connect("mem://local-forward").unwrap();
        assert!(targets(&mut conn, "sentori@x.test").unwrap().is_empty());
        set_targets(
            &mut conn,
            "Sentori@x.test",
            &["Lihao@x.test".into(), "b@x.test".into()],
        )
        .unwrap();
        assert_eq!(
            targets(&mut conn, "sentori@x.test").unwrap(),
            vec!["b@x.test", "lihao@x.test"]
        );
        set_targets(&mut conn, "sentori@x.test", &["lihao@x.test".into()]).unwrap();
        assert_eq!(
            targets(&mut conn, "sentori@x.test").unwrap(),
            vec!["lihao@x.test"]
        );
        set_targets(&mut conn, "sentori@x.test", &[]).unwrap();
        assert!(targets(&mut conn, "sentori@x.test").unwrap().is_empty());
    }
}
