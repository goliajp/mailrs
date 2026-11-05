//! The Sent copies a core filed for mail its user submitted over SMTP,
//! until the client files its own.
//!
//! ```text
//! mailrs:submitted:{user}   hash — Message-ID → "1", each field kept KEPT
//! ```
//!
//! The copy is filed from what the client submitted, which leaves out what
//! a client keeps only in its own copy, a `Bcc:` above all. A client that
//! saves its copy to Sent by IMAP APPEND afterwards has that copy replace
//! this one, once; a later APPEND of the same Message-ID is answered with
//! the message already there.

use std::io;
use std::time::Duration;

use kevy_client::{Connection, HExpireCond};

/// How long a client has to file its own copy: one that has been offline
/// for a week still does.
pub const KEPT: Duration = Duration::from_secs(7 * 86_400);

pub fn key(user: &str) -> String {
    format!("mailrs:submitted:{user}")
}

/// `user`'s Sent copy of `message_id` was filed for their SMTP submission.
pub fn mark(c: &mut Connection, user: &str, message_id: &str) -> io::Result<()> {
    let key = key(user);
    c.hset(key.as_bytes(), &[(message_id.as_bytes(), b"1".as_slice())])?;
    c.hexpire(
        key.as_bytes(),
        &[message_id.as_bytes()],
        KEPT,
        HExpireCond::Always,
    )?;
    Ok(())
}

/// Whether `user`'s Sent copy of `message_id` is still the one filed for
/// their submission.
pub fn is_marked(c: &mut Connection, user: &str, message_id: &str) -> io::Result<bool> {
    Ok(c.hget(key(user).as_bytes(), message_id.as_bytes())?
        .is_some())
}

/// The client's own copy replaced it.
pub fn clear(c: &mut Connection, user: &str, message_id: &str) -> io::Result<()> {
    c.hdel(key(user).as_bytes(), &[message_id.as_bytes()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_lasts_until_cleared_and_names_one_user_and_message() {
        let mut c = Connection::connect("mem://families-submitted").unwrap();
        mark(&mut c, "a@x.test", "m1@x.test").unwrap();
        assert!(is_marked(&mut c, "a@x.test", "m1@x.test").unwrap());
        assert!(!is_marked(&mut c, "b@x.test", "m1@x.test").unwrap());
        assert!(!is_marked(&mut c, "a@x.test", "m2@x.test").unwrap());
        clear(&mut c, "a@x.test", "m1@x.test").unwrap();
        assert!(!is_marked(&mut c, "a@x.test", "m1@x.test").unwrap());
    }
}
