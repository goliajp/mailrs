//! The DKIM keys the sender actually loaded, published for the domain
//! check.
//!
//! ```text
//! mailrs:sender:dkim-signers   string — JSON {default_domain, selectors, published_at}
//! ```
//!
//! Written by the sender once per start, after it has built its signers.
//! Read by webapi, which cannot open the key files and so cannot tell from
//! the environment alone whether a key loaded. A domain is in `selectors`
//! only when its key parsed; the default domain is there when the default
//! key parsed.
//!
//! One JSON value rather than a hash so that a start replaces the whole set
//! in a single SET: the embedded backend the tests run on has no MULTI or
//! RENAME, and a DEL followed by an HSET would let a reader see nothing in
//! between.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const KEY: &[u8] = b"mailrs:sender:dkim-signers";

/// What the sender can sign.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DkimSigners {
    /// the configured default `d=`, used for mail from a domain with no key
    /// of its own; `None` when the sender has no DKIM
    pub default_domain: Option<String>,
    /// every domain whose key parsed → its selector
    pub selectors: HashMap<String, String>,
}

/// [`DkimSigners`] as read back, with when the sender wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Published {
    #[serde(flatten)]
    pub signers: DkimSigners,
    pub published_at: i64,
}

/// Replace the published set with `signers`.
///
/// # Errors
/// The store could not be written.
pub fn publish(
    conn: &mut kevy_client::Connection,
    signers: &DkimSigners,
    now: i64,
) -> std::io::Result<()> {
    let value = serde_json::to_vec(&Published {
        signers: signers.clone(),
        published_at: now,
    })?;
    conn.set(KEY, &value)?;
    Ok(())
}

/// The published set; `None` when the sender has not written it.
///
/// # Errors
/// The store could not be read, or holds something that is not this
/// shape — both distinct from never published.
pub fn read(conn: &mut kevy_client::Connection) -> std::io::Result<Option<Published>> {
    match conn.get(KEY)? {
        Some(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signers(default: Option<&str>, pairs: &[(&str, &str)]) -> DkimSigners {
        DkimSigners {
            default_domain: default.map(str::to_string),
            selectors: pairs
                .iter()
                .map(|(d, s)| (d.to_string(), s.to_string()))
                .collect(),
        }
    }

    #[test]
    fn nothing_published_reads_as_none() {
        let mut c = kevy_client::Connection::connect("mem://dkim-signers-none").unwrap();
        assert_eq!(read(&mut c).unwrap(), None);
    }

    #[test]
    fn a_publish_replaces_the_previous_one_whole() {
        let mut c = kevy_client::Connection::connect("mem://dkim-signers-replace").unwrap();
        let first = signers(
            Some("golia.jp"),
            &[("golia.jp", "mail"), ("golia.ai", "s2")],
        );
        publish(&mut c, &first, 100).unwrap();
        assert_eq!(
            read(&mut c).unwrap(),
            Some(Published {
                signers: first,
                published_at: 100
            })
        );

        let second = signers(Some("golia.jp"), &[("golia.jp", "mail")]);
        publish(&mut c, &second, 200).unwrap();
        let back = read(&mut c).unwrap().unwrap();
        assert_eq!(back.signers, second, "golia.ai no longer loads");
        assert_eq!(back.published_at, 200);
    }

    #[test]
    fn a_sender_without_dkim_still_publishes_that_it_signs_nothing() {
        let mut c = kevy_client::Connection::connect("mem://dkim-signers-off").unwrap();
        publish(&mut c, &DkimSigners::default(), 300).unwrap();
        let back = read(&mut c).unwrap().expect("published");
        assert_eq!(back.signers, DkimSigners::default());
        assert_eq!(back.published_at, 300);
    }
}
