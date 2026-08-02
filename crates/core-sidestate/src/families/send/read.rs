//! Reads over a send row, and the thread repoint.

//! One row per send, so the Send view can be a projection of the thing
//! that actually happened rather than a mirror of a copy:
//!
//! ```text
//! mailrs:send:{user}:{send_id}         hash — the send
//! mailrs:send:{user}:{send_id}:rcpt    hash — recipient → state
//! mailrs:send:{user}:index             zset — send_id → created_at
//! mailrs:send:{user}:by_status:{s}     zset — send_id → created_at
//! ```
//!
//! ## Why this exists
//!
//! Sending does two writes. `enqueue_outbound_at` is the send: it is
//! synchronous, it propagates its error, and if it fails the mail is not
//! sent. `mirror_send_to_sender_view` writes the thread copy the Sent
//! view reads, and it is best-effort in every branch — a failed maildir
//! write falls back to a synthetic blob_ref, a failed serialize
//! `return`s, a failed `deliver_message` logs a warning. It returns
//! `()`.
//!
//! So the fact "I sent this" was recorded by the operation allowed to
//! fail silently, while the operation that cannot fail was not consulted
//! by the view. On 2026-07-30 a mail with 15 MB of attachments delivered
//! fine (`250 2.0.0 OK` from lgesmtp.lge.com, 24s after submission) and
//! took 1m42s to appear in Sent — the list answering in 2–5 ms each time
//! it was asked, and simply not knowing. Five candidate mechanisms were
//! each ruled out by evidence and the specific one was never identified.
//! It does not need to be: a row written in the same fallible step as
//! the enqueue cannot be missing from a send that returned 200.
//!
//! ## Per recipient, because a send is not one outcome
//!
//! The queue already holds one job per recipient. One envelope to three
//! recipients can be accepted by two MXs and 5xx'd by the third, and
//! reporting that as either "delivered" or "failed" is a lie the user
//! acts on. The message-level status is derived from the recipient rows
//! ([`Status::derive`]), never stored as an independent fact.
//!
//! Remote responses are kept verbatim. When a send fails, the receiving
//! server's own words are the only useful thing on the screen, and
//! flattening them into "delivery failed" is what makes a support
//! conversation impossible.

use super::*;
use std::collections::BTreeMap;

pub(crate) fn current_status(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<Option<Status>> {
    let raw = conn
        .hget(send_key(user, send_id).as_bytes(), b"status")
        .map_err(std::io::Error::other)?;
    Ok(raw
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| Status::parse(&s)))
}

pub(crate) fn created_at(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<i64> {
    Ok(conn
        .hget(send_key(user, send_id).as_bytes(), b"created_at")
        .map_err(std::io::Error::other)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

/// Read one send's recipient states.
pub fn read_recipients(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<Vec<RecipientState>> {
    let flat = conn
        .hgetall(rcpt_key(user, send_id).as_bytes())
        .map_err(std::io::Error::other)?;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let recipient = String::from_utf8_lossy(&flat[i]).to_string();
        let raw = String::from_utf8_lossy(&flat[i + 1]).to_string();
        if let Some(state) = RecipientState::decode(&recipient, &raw) {
            out.push(state);
        }
        i += 2;
    }
    // Deterministic order so a page does not reshuffle between reads.
    out.sort_by(|a, b| a.recipient.cmp(&b.recipient));
    Ok(out)
}

/// Field map for one send, or empty when it does not exist.
pub fn read_send(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<BTreeMap<String, String>> {
    let flat = conn
        .hgetall(send_key(user, send_id).as_bytes())
        .map_err(std::io::Error::other)?;
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        out.insert(
            String::from_utf8_lossy(&flat[i]).to_string(),
            String::from_utf8_lossy(&flat[i + 1]).to_string(),
        );
        i += 2;
    }
    Ok(out)
}

/// The thread id a row should hold, or `None` when it already holds it.
///
/// `None` for "no change needed" rather than returning the current value:
/// a write that stores what is already there costs a round trip and makes
/// the reported count a lie about how much moved
/// (`periodic-work-must-converge`).
fn next_thread_id<'a>(
    current: &str,
    merged: &'a std::collections::HashMap<String, String>,
) -> Option<&'a str> {
    let canonical = merged.get(current)?;
    if canonical == current {
        return None;
    }
    Some(canonical.as_str())
}

/// Re-point Send rows whose conversation was merged away.
///
/// `merged` maps a thread id that no longer exists to the canonical thread
/// that absorbed it. Returns the number of rows changed.
///
/// `SendRow.thread_id` is written once, at enqueue. Thread ids are not
/// stable: the rethread pass merges conversations and the absorbed id stops
/// existing. A row still holding one navigates to an empty thread — on
/// 2026-07-30 a repaired reply showed "(no subject)" in Send for exactly
/// this reason, because the Send list trusted the snapshot while
/// `list_sent_messages` used the live tid.
///
/// Maintained rather than resolved on read: the merge knows precisely which
/// ids died, so this runs once per merge instead of a lookup per row per
/// list render (`kevy/every-writer-maintains-the-row`).
pub fn repoint_threads(
    conn: &mut kevy_client::Connection,
    user: &str,
    merged: &std::collections::HashMap<String, String>,
) -> std::io::Result<u64> {
    if merged.is_empty() {
        return Ok(0);
    }
    let ids = conn
        .zrange(index_key(user).as_bytes(), 0, -1)
        .map_err(std::io::Error::other)?;
    let mut changed = 0u64;
    for raw in ids {
        let Ok(send_id) = String::from_utf8(raw) else {
            continue;
        };
        let key = send_key(user, &send_id);
        let current = conn
            .hget(key.as_bytes(), b"thread_id")
            .map_err(std::io::Error::other)?
            .and_then(|v| String::from_utf8(v).ok());
        let Some(current) = current else {
            continue;
        };
        let Some(canonical) = next_thread_id(&current, merged) else {
            continue;
        };
        conn.hset(
            key.as_bytes(),
            &[(b"thread_id" as &[u8], canonical.as_bytes())],
        )
        .map_err(std::io::Error::other)?;
        changed += 1;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rcpt(name: &str, delivered: bool, pending: bool, code: u16) -> RecipientState {
        RecipientState {
            recipient: name.to_string(),
            delivered,
            pending,
            code,
            message: String::new(),
        }
    }

    #[test]
    fn a_send_with_nothing_attempted_yet_is_sending() {
        assert_eq!(Status::derive(&[]), Status::Sending);
        assert_eq!(
            Status::derive(&[RecipientState::queued("a@x.com")]),
            Status::Sending
        );
    }

    #[test]
    fn all_accepted_is_delivered_and_none_accepted_is_failed() {
        assert_eq!(
            Status::derive(&[
                rcpt("a@x.com", true, false, 250),
                rcpt("b@x.com", true, false, 250)
            ]),
            Status::Delivered
        );
        assert_eq!(
            Status::derive(&[rcpt("a@x.com", false, false, 550)]),
            Status::Failed
        );
    }

    /// The case that makes `Partial` necessary rather than cosmetic.
    #[test]
    fn some_accepted_and_some_rejected_is_partial() {
        let r = [
            rcpt("a@x.com", true, false, 250),
            rcpt("b@x.com", true, false, 250),
            rcpt("c@x.com", false, false, 550),
        ];
        assert_eq!(Status::derive(&r), Status::Partial);
    }

    /// A send with attempts left is unfinished, not partially failed.
    /// Showing a verdict early invites a resend of mail that was about
    /// to arrive.
    #[test]
    fn one_still_in_flight_keeps_the_whole_send_sending() {
        let r = [
            rcpt("a@x.com", true, false, 250),
            rcpt("b@x.com", false, true, 0),
        ];
        assert_eq!(Status::derive(&r), Status::Sending);
    }

    #[test]
    fn a_recipient_state_round_trips() {
        let s = RecipientState {
            recipient: "a@x.com".into(),
            delivered: true,
            pending: false,
            code: 250,
            message: "2.0.0 OK".into(),
        };
        assert_eq!(RecipientState::decode("a@x.com", &s.encode()), Some(s));
    }

    /// Remote queue ids contain colons. Only the first three separators
    /// are structural, or the message gets truncated at the first one.
    #[test]
    fn a_remote_message_containing_colons_survives() {
        let s = RecipientState {
            recipient: "wonil01.lee@lge.com".into(),
            delivered: true,
            pending: false,
            code: 250,
            message: "2.0.0 OK 3C/A4-19206-A8F6A6A6: queued as ABC:123".into(),
        };
        let back = RecipientState::decode("wonil01.lee@lge.com", &s.encode()).unwrap();
        assert_eq!(back.message, s.message);
        assert_eq!(back.code, 250);
    }

    #[test]
    fn status_strings_round_trip() {
        for s in [
            Status::Scheduled,
            Status::Sending,
            Status::Delivered,
            Status::Failed,
            Status::Partial,
        ] {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        assert_eq!(Status::parse("nonsense"), None);
    }

    /// A transient failure with retries left is not a verdict.
    ///
    /// The sender computes `pending` this way; asserting it here keeps the
    /// rule in one place rather than trusting the caller. A send shown as
    /// failed while a retry is queued invites resending mail that is
    /// about to arrive.
    #[test]
    fn a_transient_failure_with_retries_left_keeps_the_send_in_flight() {
        let with_retries = RecipientState {
            recipient: "a@x.com".into(),
            delivered: false,
            pending: true,
            code: 451,
            message: "4.7.1 try again later".into(),
        };
        assert_eq!(
            Status::derive(std::slice::from_ref(&with_retries)),
            Status::Sending
        );

        let exhausted = RecipientState {
            pending: false,
            ..with_retries
        };
        assert_eq!(Status::derive(&[exhausted]), Status::Failed);
    }

    /// The failure text is what a support conversation runs on, so the
    /// code is duplicated into its own field rather than replacing it.
    #[test]
    fn a_rejection_keeps_both_the_code_and_the_remotes_words() {
        let s = RecipientState {
            recipient: "a@x.com".into(),
            delivered: false,
            pending: false,
            code: 550,
            message: "5.1.1 <a@x.com>: Recipient address rejected: User unknown".into(),
        };
        let back = RecipientState::decode("a@x.com", &s.encode()).unwrap();
        assert_eq!(back.code, 550);
        assert!(back.message.contains("User unknown"));
    }

    #[test]
    fn keys_are_scoped_per_user_and_send() {
        assert_eq!(send_key("u@x.com", "m1"), "mailrs:send:u@x.com:m1");
        assert_eq!(rcpt_key("u@x.com", "m1"), "mailrs:send:u@x.com:m1:rcpt");
        assert_eq!(index_key("u@x.com"), "mailrs:send:u@x.com:index");
        assert_eq!(
            by_status_key("u@x.com", Status::Failed),
            "mailrs:send:u@x.com:by_status:failed"
        );
    }
}

#[cfg(test)]
mod repoint_tests {
    use super::next_thread_id;
    use std::collections::HashMap;

    fn merged(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    /// The 2026-07-30 case: a reply's own thread was absorbed into the
    /// conversation it belonged to, and its Send row still named the dead
    /// one — so Send opened an empty thread and showed "(no subject)".
    #[test]
    fn a_row_naming_a_merged_away_thread_moves_to_the_canonical_one() {
        let m = merged(&[(
            "9d8549f828cd6aea@golia.jp",
            "2d31fbc5-f7f1-4958-9e81-99819ab73d61@nagatax.tokyo.jp",
        )]);
        assert_eq!(
            next_thread_id("9d8549f828cd6aea@golia.jp", &m),
            Some("2d31fbc5-f7f1-4958-9e81-99819ab73d61@nagatax.tokyo.jp")
        );
    }

    /// Untouched conversations must not be written. A sweep that rewrites
    /// rows it did not need to touch reports work it did not do, which is
    /// the reporting defect this whole phase exists to remove.
    #[test]
    fn a_row_whose_thread_was_not_merged_is_left_alone() {
        let m = merged(&[("dead@x.com", "alive@x.com")]);
        assert_eq!(next_thread_id("unrelated@x.com", &m), None);
        assert_eq!(next_thread_id("", &m), None);
    }

    /// A thread mapped to itself is not a change. Guarded because a
    /// self-mapping is exactly what a re-run of an idempotent merge could
    /// produce, and rewriting on every run would make the count meaningless.
    #[test]
    fn a_thread_mapped_to_itself_is_not_a_change() {
        let m = merged(&[("same@x.com", "same@x.com")]);
        assert_eq!(next_thread_id("same@x.com", &m), None);
    }

    #[test]
    fn an_empty_map_changes_nothing() {
        assert_eq!(next_thread_id("any@x.com", &merged(&[])), None);
    }
}
