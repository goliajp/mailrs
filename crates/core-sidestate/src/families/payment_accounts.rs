//! The payment accounts each sender has given a reader, across
//! conversations.
//!
//! ```text
//! mailrs:payacct:{user}:{sender}   hash   account key -> "{arrival} {message_id}"
//!                                         last        -> latest arrival recorded
//! ```
//!
//! Built like [`super::callback_numbers`]: one hash per reader and sender
//! address, a field per account, its value the earliest message that gave
//! it, at most [`MAX_PER_SENDER`] accounts (the earliest kept), the hash
//! expiring [`KEEP_DAYS`] after the latest message recorded into it, and
//! recording converges. A field is an account's key
//! ([`mailrs_fraud::payment::PaymentIds::account_keys`]): a hash of an IBAN
//! or of a Japanese account's bank and number, never the number itself.
//!
//! The receive path (the message's text) and fastcore's attachment reader
//! (the attachments' text) write; the fraud facts read it, for whether a
//! sender who gave the reader one account now gives another
//! ([`changed`]).

use std::io;

use kevy_client::Connection;

use super::callback_numbers::{read, record_at};

const PREFIX: &str = "mailrs:payacct:";

/// Accounts kept per reader and sender.
pub const MAX_PER_SENDER: usize = 16;

/// Days a sender's accounts are kept after the last message that gave any.
pub const KEEP_DAYS: u64 = 400;

/// The hash of the accounts `sender` gave `user`. Both lowercased.
pub fn key(user: &str, sender: &str) -> String {
    format!(
        "{PREFIX}{}:{}",
        user.trim().to_ascii_lowercase(),
        sender.trim().to_ascii_lowercase()
    )
}

/// Record that `message_id`, from `from` (the decoded header) to `user` and
/// arrived at `at`, gave the accounts whose keys are `keys`. Answers how
/// many were new or moved to an earlier message; 0 when nothing was
/// written.
pub fn record(
    conn: &mut Connection,
    user: &str,
    from: &str,
    keys: &[String],
    at: i64,
    message_id: &str,
) -> io::Result<usize> {
    let sender = super::correspondent::first_address(from);
    if keys.is_empty() || user.trim().is_empty() || sender.trim().is_empty() {
        return Ok(0);
    }
    record_at(
        conn,
        &key(user, &sender),
        keys,
        at,
        message_id,
        MAX_PER_SENDER,
        KEEP_DAYS,
    )
}

/// The keys of the accounts the sender of `from` gave `user` in messages
/// that arrived before `at`.
pub fn known_before(
    conn: &mut Connection,
    user: &str,
    from: &str,
    at: i64,
) -> io::Result<Vec<String>> {
    let sender = super::correspondent::first_address(from);
    let (held, _) = read(conn, &key(user, &sender))?;
    Ok(held
        .into_iter()
        .filter(|g| g.first_seen < at)
        .map(|g| g.number)
        .collect())
}

/// Whether the message arrived at `at` from `from`, carrying the accounts
/// `keys`, gives `user` an account the sender had not given before, from a
/// sender that had given another one. No accounts in the message, or none
/// known before it, is not a change.
pub fn changed(
    conn: &mut Connection,
    user: &str,
    from: &str,
    keys: &[String],
    at: i64,
) -> io::Result<bool> {
    if keys.is_empty() {
        return Ok(false);
    }
    let before = known_before(conn, user, from, at)?;
    Ok(!before.is_empty() && keys.iter().any(|k| !before.contains(k)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        Connection::connect(&format!("mem://payacct-{name}")).unwrap()
    }

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const FROM: &str = "Billing <Billing@Shop.example>";

    #[test]
    fn a_new_account_from_a_sender_with_one_on_record_is_a_change() {
        let mut c = conn("change");
        record(
            &mut c,
            "bob@x.example",
            FROM,
            &keys(&["k1"]),
            100,
            "<a@shop>",
        )
        .unwrap();
        assert!(changed(&mut c, "Bob@x.example", FROM, &keys(&["k2"]), 200).unwrap());
        assert!(!changed(&mut c, "bob@x.example", FROM, &keys(&["k1"]), 200).unwrap());
        // nothing on record before the message: no change to speak of
        assert!(!changed(&mut c, "bob@x.example", FROM, &keys(&["k2"]), 100).unwrap());
        assert!(!changed(&mut c, "bob@x.example", FROM, &keys(&[]), 200).unwrap());
        // another reader, another sender: their own histories
        assert!(!changed(&mut c, "eve@x.example", FROM, &keys(&["k2"]), 200).unwrap());
        assert!(
            !changed(
                &mut c,
                "bob@x.example",
                "other@shop.example",
                &keys(&["k2"]),
                200
            )
            .unwrap()
        );
    }

    #[test]
    fn judged_after_its_own_recording_a_message_is_judged_on_what_came_before() {
        let mut c = conn("after");
        record(
            &mut c,
            "bob@x.example",
            FROM,
            &keys(&["k1"]),
            100,
            "<a@shop>",
        )
        .unwrap();
        record(
            &mut c,
            "bob@x.example",
            FROM,
            &keys(&["k2"]),
            200,
            "<b@shop>",
        )
        .unwrap();
        // a sweep reads stored mail, already recorded
        assert!(changed(&mut c, "bob@x.example", FROM, &keys(&["k2"]), 200).unwrap());
        assert!(!changed(&mut c, "bob@x.example", FROM, &keys(&["k1"]), 100).unwrap());
    }

    #[test]
    fn recording_the_same_message_twice_writes_nothing_the_second_time() {
        let mut c = conn("twice");
        let first = record(
            &mut c,
            "bob@x.example",
            FROM,
            &keys(&["k1"]),
            100,
            "<a@shop>",
        );
        let again = record(
            &mut c,
            "bob@x.example",
            FROM,
            &keys(&["k1"]),
            100,
            "<a@shop>",
        );
        assert_eq!((first.unwrap(), again.unwrap()), (1, 0));
        assert_eq!(
            known_before(&mut c, "bob@x.example", FROM, 101).unwrap(),
            keys(&["k1"])
        );
        assert_eq!(
            key("Bob@X.example", "billing@shop.example"),
            key("bob@x.example", "Billing@Shop.example")
        );
    }
}
