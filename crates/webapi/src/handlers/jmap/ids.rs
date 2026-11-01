//! JMAP ids for this server's objects.
//!
//! RFC 8620 §1.2: an Id is 1–255 characters of `A-Za-z0-9_-`. A message is
//! named by the per-user uid both cores resolve (`messages/by-uid`), which
//! a move between mailboxes does not change. A thread is named by a 64-bit
//! hash of its id, fixed width, so the record of what a mailbox held can
//! keep it in eight bytes and still name a thread that is gone.

/// `M{uid}`.
pub fn email(uid: u32) -> String {
    format!("M{uid}")
}

pub fn parse_email(id: &str) -> Option<u32> {
    id.strip_prefix('M')?.parse().ok().filter(|u| *u > 0)
}

/// The key a thread is known by.
pub fn thread_key(thread_id: &str) -> u64 {
    mailrs_core_api::method::message_index::fnv64(thread_id.as_bytes())
}

/// `T` + the thread's key in hex.
pub fn thread(key: u64) -> String {
    format!("T{key:016x}")
}

pub fn parse_thread(id: &str) -> Option<u64> {
    let hex = id.strip_prefix('T').filter(|h| h.len() == 16)?;
    u64::from_str_radix(hex, 16).ok()
}

/// A blob: a whole message `B{uid}`, one of its parts `B{uid}-{n}`, or an
/// upload `U{sha256}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blob {
    Message(u32),
    Part(u32, usize),
    Upload(String),
}

pub fn blob(b: &Blob) -> String {
    match b {
        Blob::Message(uid) => format!("B{uid}"),
        Blob::Part(uid, n) => format!("B{uid}-{n}"),
        Blob::Upload(sha) => format!("U{sha}"),
    }
}

pub fn parse_blob(id: &str) -> Option<Blob> {
    if let Some(sha) = id.strip_prefix('U') {
        let hex = sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit());
        return hex.then(|| Blob::Upload(sha.to_ascii_lowercase()));
    }
    let rest = id.strip_prefix('B')?;
    match rest.split_once('-') {
        Some((uid, n)) => Some(Blob::Part(uid.parse().ok()?, n.parse().ok()?)),
        None => Some(Blob::Message(rest.parse().ok()?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_jmap_ids_and_round_trip() {
        let key = thread_key("t-hello@partner.test");
        let t = thread(key);
        assert!(t.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_eq!(parse_thread(&t), Some(key));
        assert_eq!(parse_thread("Tzz"), None);
        assert_eq!(parse_email(&email(42)), Some(42));
        assert_eq!(parse_email("M0"), None);
        assert_eq!(parse_email("msg-4"), None);
        for b in [
            Blob::Message(7),
            Blob::Part(7, 2),
            Blob::Upload("ab".repeat(32)),
        ] {
            assert_eq!(parse_blob(&blob(&b)), Some(b));
        }
        assert_eq!(parse_blob("Uzz"), None);
        assert_eq!(parse_blob("X1"), None);
    }
}
