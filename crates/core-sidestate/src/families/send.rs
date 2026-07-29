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

use std::collections::BTreeMap;

/// Message-level state, derived from the recipient rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// A future `scheduled_at` that has not come due.
    Scheduled,
    /// Queued or in flight.
    Sending,
    /// Every recipient accepted.
    Delivered,
    /// Every recipient permanently rejected, or retries exhausted.
    Failed,
    /// Some accepted, some did not. Not decoration: calling this either
    /// of the two above would misinform the person deciding whether to
    /// resend, and to whom.
    Partial,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Scheduled => "scheduled",
            Status::Sending => "sending",
            Status::Delivered => "delivered",
            Status::Failed => "failed",
            Status::Partial => "partial",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "scheduled" => Some(Status::Scheduled),
            "sending" => Some(Status::Sending),
            "delivered" => Some(Status::Delivered),
            "failed" => Some(Status::Failed),
            "partial" => Some(Status::Partial),
            _ => None,
        }
    }

    /// The message-level status implied by its recipients.
    ///
    /// Anything still in flight keeps the whole send `Sending` — a
    /// partially-complete send is not `Partial`, it is unfinished, and
    /// showing a verdict before the attempts are over invites a resend
    /// of mail that was about to arrive.
    pub fn derive(recipients: &[RecipientState]) -> Status {
        if recipients.is_empty() {
            return Status::Sending;
        }
        if recipients.iter().any(|r| r.is_in_flight()) {
            return Status::Sending;
        }
        let delivered = recipients.iter().filter(|r| r.delivered).count();
        if delivered == recipients.len() {
            Status::Delivered
        } else if delivered == 0 {
            Status::Failed
        } else {
            Status::Partial
        }
    }
}

/// One recipient's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientState {
    pub recipient: String,
    /// `true` once an MX returned 2xx.
    pub delivered: bool,
    /// `true` while attempts remain — queued, in flight, or waiting on
    /// backoff.
    pub pending: bool,
    /// The remote's status code, verbatim (`250`, `550`, …). `0` when no
    /// server has answered yet.
    pub code: u16,
    /// The remote's text, verbatim. Not paraphrased.
    pub message: String,
}

impl RecipientState {
    /// A fresh recipient, before any MX has been contacted.
    pub fn queued(recipient: &str) -> Self {
        Self {
            recipient: recipient.to_string(),
            delivered: false,
            pending: true,
            code: 0,
            message: String::new(),
        }
    }

    pub fn is_in_flight(&self) -> bool {
        self.pending && !self.delivered
    }

    /// Stored as `delivered:pending:code:message`. The message can
    /// contain colons — `3C/A4-19206-A8F6A6A6` style queue ids are
    /// common — so only the first three separators are structural.
    pub fn encode(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            u8::from(self.delivered),
            u8::from(self.pending),
            self.code,
            self.message
        )
    }

    pub fn decode(recipient: &str, raw: &str) -> Option<Self> {
        let mut it = raw.splitn(4, ':');
        let delivered = it.next()? == "1";
        let pending = it.next()? == "1";
        let code = it.next()?.parse().ok()?;
        let message = it.next().unwrap_or("").to_string();
        Some(Self {
            recipient: recipient.to_string(),
            delivered,
            pending,
            code,
            message,
        })
    }
}

/// The send as the Send view needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRow {
    pub send_id: String,
    pub message_id: String,
    pub thread_id: String,
    pub subject: String,
    pub to_csv: String,
    pub cc_csv: String,
    pub created_at: i64,
    pub status: Status,
    /// Maildir blob holding the RFC 5322 bytes. Resend re-enqueues
    /// these; re-edit parses them back into compose fields. Keeping the
    /// bytes rather than the compose form is what makes attachments
    /// survive both.
    pub envelope_ref: String,
    /// Set when this send is a resend of an earlier one, which keeps its
    /// own row. A failed send is history, and history is the reason
    /// anyone opens this screen.
    pub resent_from: Option<String>,
}

pub fn send_key(user: &str, send_id: &str) -> String {
    format!("mailrs:send:{user}:{send_id}")
}

pub fn rcpt_key(user: &str, send_id: &str) -> String {
    format!("mailrs:send:{user}:{send_id}:rcpt")
}

pub fn index_key(user: &str) -> String {
    format!("mailrs:send:{user}:index")
}

pub fn by_status_key(user: &str, status: Status) -> String {
    format!("mailrs:send:{user}:by_status:{}", status.as_str())
}

/// Record a send and its recipients.
///
/// Returns `Err` on any failure, and the caller must propagate it: a
/// send whose row cannot be written must not report success. That is the
/// whole point — the alternative is the best-effort mirror this replaces.
pub fn write_send(
    conn: &mut kevy_client::Connection,
    user: &str,
    row: &SendRow,
    recipients: &[String],
) -> std::io::Result<()> {
    let states: Vec<RecipientState> = recipients
        .iter()
        .map(|r| RecipientState::queued(r.trim()))
        .filter(|r| !r.recipient.is_empty())
        .collect();
    let status = if row.status == Status::Scheduled {
        Status::Scheduled
    } else {
        Status::derive(&states)
    };

    let created = row.created_at.to_string();
    let key = send_key(user, &row.send_id);
    let fields: Vec<(&[u8], &[u8])> = vec![
        (b"send_id", row.send_id.as_bytes()),
        (b"message_id", row.message_id.as_bytes()),
        (b"thread_id", row.thread_id.as_bytes()),
        (b"subject", row.subject.as_bytes()),
        (b"to_csv", row.to_csv.as_bytes()),
        (b"cc_csv", row.cc_csv.as_bytes()),
        (b"created_at", created.as_bytes()),
        (b"status", status.as_str().as_bytes()),
        (b"envelope_ref", row.envelope_ref.as_bytes()),
        (
            b"resent_from",
            row.resent_from.as_deref().unwrap_or("").as_bytes(),
        ),
    ];
    conn.hset(key.as_bytes(), &fields)
        .map_err(std::io::Error::other)?;

    if !states.is_empty() {
        let encoded: Vec<(String, String)> = states
            .iter()
            .map(|s| (s.recipient.clone(), s.encode()))
            .collect();
        let pairs: Vec<(&[u8], &[u8])> = encoded
            .iter()
            .map(|(r, v)| (r.as_bytes(), v.as_bytes()))
            .collect();
        conn.hset(rcpt_key(user, &row.send_id).as_bytes(), &pairs)
            .map_err(std::io::Error::other)?;
    }

    let score = row.created_at as f64;
    conn.zadd(
        index_key(user).as_bytes(),
        &[(score, row.send_id.as_bytes())],
    )
    .map_err(std::io::Error::other)?;
    conn.zadd(
        by_status_key(user, status).as_bytes(),
        &[(score, row.send_id.as_bytes())],
    )
    .map_err(std::io::Error::other)?;
    Ok(())
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
