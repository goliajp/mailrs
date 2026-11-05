//! The addresses each user has sent mail to, derived from the
//! relationship records ([`super::contacts`]):
//!   `mailrs:contact_sent:{user}`   set: every `{email}` whose
//!                                  `mailrs:contact:{user}:{email}` has a
//!                                  `sent_count` above zero
//!
//! The fraud rules read it to know whom a user wrote to without scanning
//! every record. [`super::contacts::record_sent_to`] and [`set_counts`] write
//! `sent_count` and this set together; [`rebuild_sent_to`] derives the set
//! afresh from the records.

use mailrs_core_api::method::admin::SentToBackfillResponse;

use super::contacts::{CONTACT_PREFIX, contact_key};

/// The addresses `user` has sent mail to: those whose `sent_count` is
/// above zero. Spelled with the same `user` as [`contact_key`].
pub fn sent_to_key(user: &str) -> String {
    format!("mailrs:contact_sent:{user}")
}

/// Overwrite `user`'s counts for `email` with ones recounted from stored
/// mail, keeping [`sent_to_key`] in step with the new `sent_count`.
pub fn set_counts(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    received: u64,
    sent: u64,
) -> std::io::Result<()> {
    let key = contact_key(user, email);
    let (recv, sent_s) = (received.to_string(), sent.to_string());
    let pairs: [(&[u8], &[u8]); 2] = [
        (b"received_count", recv.as_bytes()),
        (b"sent_count", sent_s.as_bytes()),
    ];
    conn.hset(key.as_bytes(), &pairs)?;
    let set = sent_to_key(user);
    match sent {
        0 => conn.srem(set.as_bytes(), &[email.as_bytes()])?,
        _ => conn.sadd(set.as_bytes(), &[email.as_bytes()])?,
    };
    Ok(())
}

/// Every address `user` has sent mail to, read from [`sent_to_key`].
pub fn sent_to(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<String>> {
    Ok(conn
        .smembers(sent_to_key(user).as_bytes())?
        .into_iter()
        .map(|m| String::from_utf8_lossy(&m).into_owned())
        .collect())
}

/// Derive every user's [`sent_to_key`] set from the relationship hashes,
/// adding each address whose `sent_count` is above zero. Adds only, so a
/// second run finds nothing missing; `dry` counts without writing. Both
/// cores answer `POST /v1/admin/maintenance:backfill-sent-to` with it.
pub fn rebuild_sent_to(
    conn: &mut kevy_client::Connection,
    dry: bool,
) -> std::io::Result<SentToBackfillResponse> {
    let mut out = SentToBackfillResponse {
        dry_run: dry,
        ..SentToBackfillResponse::default()
    };
    let pattern = format!("{CONTACT_PREFIX}*");
    let mut cursor = 0u64;
    loop {
        let (next, keys) = conn.scan(cursor, Some(pattern.as_bytes()), Some(1000))?;
        for raw in keys {
            let key = String::from_utf8_lossy(&raw).into_owned();
            let Some((user, email)) = key
                .strip_prefix(CONTACT_PREFIX)
                .and_then(|rest| rest.split_once(':'))
            else {
                continue;
            };
            out.records += 1;
            if sent_count(conn, &key) == 0 {
                continue;
            }
            out.sent_to += 1;
            let set = sent_to_key(user);
            if conn.sismember(set.as_bytes(), email.as_bytes())? {
                continue;
            }
            out.missing += 1;
            if !dry {
                conn.sadd(set.as_bytes(), &[email.as_bytes()])?;
            }
        }
        if next == 0 {
            break;
        }
        cursor = next;
    }
    Ok(out)
}

fn sent_count(conn: &mut kevy_client::Connection, key: &str) -> u64 {
    conn.hget(key.as_bytes(), b"sent_count")
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::contacts::vips_key;
    use super::*;

    #[test]
    fn recounted_counts_keep_the_sent_to_set_in_step() {
        let mut c = kevy_client::Connection::connect("mem://contacts-set-counts").unwrap();
        set_counts(&mut c, "me@x", "wrote@y", 1, 2).unwrap();
        set_counts(&mut c, "me@x", "heard@y", 7, 0).unwrap();
        assert_eq!(sent_to(&mut c, "me@x").unwrap(), vec!["wrote@y"]);
        set_counts(&mut c, "me@x", "wrote@y", 1, 0).unwrap();
        assert!(sent_to(&mut c, "me@x").unwrap().is_empty());
    }

    /// The set is rebuilt from `sent_count` alone: an address that only
    /// ever wrote in is not added, and a second run has nothing to do.
    #[test]
    fn the_rebuild_adds_only_addresses_written_to_and_converges() {
        let mut c = kevy_client::Connection::connect("mem://contacts-rebuild").unwrap();
        for (addr, field, value) in [
            ("wrote@y", "sent_count", "3"),
            ("heard@y", "received_count", "40"),
            ("zero@y", "sent_count", "0"),
        ] {
            c.hset(
                contact_key("me@x", addr).as_bytes(),
                &[(field.as_bytes(), value.as_bytes())],
            )
            .unwrap();
        }
        c.sadd(vips_key("me@x").as_bytes(), &[b"heard@y" as &[u8]])
            .unwrap();
        let dry = rebuild_sent_to(&mut c, true).unwrap();
        assert_eq!(
            dry,
            SentToBackfillResponse {
                dry_run: true,
                records: 3,
                sent_to: 1,
                missing: 1
            }
        );
        assert!(sent_to(&mut c, "me@x").unwrap().is_empty());
        rebuild_sent_to(&mut c, false).unwrap();
        assert_eq!(sent_to(&mut c, "me@x").unwrap(), vec!["wrote@y"]);
        assert_eq!(rebuild_sent_to(&mut c, false).unwrap().missing, 0);
    }
}
