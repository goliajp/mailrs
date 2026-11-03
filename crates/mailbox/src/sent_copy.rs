//! Whether a message is the user's own send.
//!
//! A sent copy keeps the `From:` it was written with, which can be another
//! address of the user's: a send-as address, or their old address on mail
//! imported from another mailbox. So where the copy is filed decides as
//! much as what its header says, and every store and every sweep asks the
//! same question here.

/// The Maildir++ folder sent copies are filed in.
pub const SENT_FOLDER: &str = ".Sent";

/// Whether a stored file's reference (`<folder>/<file>`, or a bare name
/// for INBOX) is filed in Sent.
pub fn is_sent_copy(blob_ref: &str) -> bool {
    blob_ref
        .strip_prefix(SENT_FOLDER)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether `user` sent the message filed as `blob_ref` with this `From:`.
pub fn sent_by(from: &str, blob_ref: &str, user: &str) -> bool {
    is_sent_copy(blob_ref) || mailrs_rfc5322::list_contains(from, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sent_copy_is_the_users_whatever_its_from_says() {
        let me = "me@example.test";
        assert!(sent_by("Me <me@example.test>", "1.M1.host", me));
        assert!(sent_by("Me <me@old.example>", ".Sent/1.M1.host", me));
        assert!(!sent_by("Erin <erin@example.test>", "1.M1.host", me));
        assert!(!sent_by("Erin <erin@example.test>", ".Junk/1.M1.host", me));
        assert!(!sent_by(
            "Erin <erin@example.test>",
            ".Sentinel/1.M1.host",
            me
        ));
    }
}
