//! Where an agent key's owner is looked up from the key alone.
//!
//! The index is stored under the sha256 of the key, never the key: a key
//! name shows up in every SCAN, and one that is the key works as a bearer
//! credential for whoever reads it. Keys are 24 random bytes, so a plain
//! digest cannot be reversed by guessing. Agent keys do not expire, so the
//! entries written under the bare key before 2026-10-30 are moved once at
//! startup rather than on use, which would leave an unused key behind
//! forever.

use kevy_client::Connection;

const PREFIX: &str = "agent:key:secret:";
const HASHED: &str = "agent:key:secret:h:";

/// The index entry for `key`.
pub(crate) fn index_key(key: &str) -> String {
    use sha2::Digest as _;
    let digest: String = sha2::Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{HASHED}{digest}")
}

/// Every index entry, for the sweeps that look an owner's keys up by value.
pub(crate) const PATTERN: &[u8] = b"agent:key:secret:*";

/// Move every entry still stored under its key to the hash of it. Returns
/// how many entries it looked at and how many it moved; a second run moves
/// none.
pub(crate) fn move_unhashed(c: &mut Connection) -> std::io::Result<(usize, usize)> {
    let entries = c.keys(PATTERN)?;
    let mut moved = 0;
    for name in &entries {
        let name = String::from_utf8_lossy(name);
        let Some(key) = name.strip_prefix(PREFIX) else {
            continue;
        };
        if name.starts_with(HASHED) {
            continue;
        }
        let Some(value) = c.get(name.as_bytes())? else {
            continue;
        };
        c.set(index_key(key).as_bytes(), &value)?;
        c.del(&[name.as_bytes()])?;
        moved += 1;
    }
    Ok((entries.len(), moved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_name_does_not_contain_the_key() {
        assert_eq!(
            index_key("abc"),
            "agent:key:secret:h:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let key = format!("mk_{}", "ab".repeat(24));
        assert!(!index_key(&key).contains(&key));
    }

    #[test]
    fn entries_under_the_key_move_once_and_keep_their_value() {
        let mut c = Connection::connect("mem://agent-key-index-move").unwrap();
        let old = "mk_0123456789abcdef0123456789abcdef0123456789abcdef";
        let new = "mk_fedcba9876543210fedcba9876543210fedcba9876543210";
        c.set(
            format!("{PREFIX}{old}").as_bytes(),
            br#"{"user":"a@x","id":1}"#,
        )
        .unwrap();
        c.set(index_key(new).as_bytes(), br#"{"user":"a@x","id":2}"#)
            .unwrap();

        assert_eq!(move_unhashed(&mut c).unwrap(), (2, 1));
        assert_eq!(
            c.get(index_key(old).as_bytes()).unwrap().as_deref(),
            Some(br#"{"user":"a@x","id":1}"#.as_slice())
        );
        assert_eq!(c.exists(&[format!("{PREFIX}{old}").as_bytes()]).unwrap(), 0);
        assert_eq!(
            move_unhashed(&mut c).unwrap(),
            (2, 0),
            "a second run moves none"
        );
    }
}
