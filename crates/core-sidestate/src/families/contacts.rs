//! Per-sender relationship records on the network kevy, which both cores
//! write and importance scoring reads:
//!   `mailrs:contact:{user}:{email}`   hash of scoring fields

use mailrs_core_api::method::contact::ContactScoring;

fn contact_key(user: &str, email: &str) -> String {
    format!("mailrs:contact:{user}:{email}")
}

fn hget_str(conn: &mut kevy_client::Connection, key: &str, field: &str) -> Option<String> {
    conn.hget(key.as_bytes(), field.as_bytes())
        .ok()
        .flatten()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
}

fn hget_bool(conn: &mut kevy_client::Connection, key: &str, field: &str) -> bool {
    hget_str(conn, key, field).as_deref() == Some("1")
}

fn hget_u32(conn: &mut kevy_client::Connection, key: &str, field: &str) -> u32 {
    hget_str(conn, key, field)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Record that `user` received a message from `email`.
///
/// Bumps `received_count` and refreshes the sender-class flags.
///
/// The counter uses HINCRBY rather than read-modify-write: two messages
/// from the same sender arriving concurrently would otherwise lose one
/// increment.
pub fn record_inbound(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    display_name: &str,
    is_mailing_list: bool,
    is_automated: bool,
) -> std::io::Result<()> {
    if is_self(user, email) {
        return Ok(());
    }
    let key = contact_key(user, email);
    let flags: [(&[u8], &[u8]); 3] = [
        (b"display_name", display_name.as_bytes()),
        (
            b"is_mailing_list",
            if is_mailing_list { b"1" } else { b"0" },
        ),
        (b"is_automated", if is_automated { b"1" } else { b"0" }),
    ];
    conn.hset(key.as_bytes(), &flags)?;
    conn.pipeline(|p| {
        p.cmd(&[b"HINCRBY", key.as_bytes(), b"received_count", b"1"]);
    })?;
    Ok(())
}

/// True when `email` is the user themselves — a message to or from
/// one's own address (a note-to-self, a multi-device sync) carries no
/// relationship, and recording it builds a self-referential contact
/// like `contact:lihao@x:lihao@x` that is meaningless as a signal.
/// Seen in prod: a self-send had left sent_count=6 on such a key.
fn is_self(user: &str, email: &str) -> bool {
    email.trim().eq_ignore_ascii_case(user.trim())
}

/// Record that `user` sent a message to `email`.
///
/// This is the counter that makes a relationship *mutual* — without it
/// `is_mutual` and `has_sent_to` can never be true, and the strongest
/// importance signals (+0.3 each) stay permanently off.
///
/// A send to oneself is not a relationship and is ignored.
pub fn record_sent_to(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
) -> std::io::Result<()> {
    if is_self(user, email) {
        return Ok(());
    }
    let key = contact_key(user, email);
    conn.pipeline(|p| {
        p.cmd(&[b"HINCRBY", key.as_bytes(), b"sent_count", b"1"]);
    })?;
    Ok(())
}

/// How the user engaged with a message from this sender.
///
/// These are the *implicit* signals — what the user did, not what they
/// declared. They are the raw material for per-user learning: a sender
/// whose mail is opened within minutes matters more than one whose mail
/// is archived unread, regardless of how bulk-ish the headers look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engagement {
    /// Thread was opened. `fast` when it happened close enough to
    /// arrival to read as "the user was waiting for this".
    Read { fast: bool },
    /// Archived while still unread — the user dismissed it unseen.
    ArchivedUnread,
    /// Explicitly starred.
    Starred,
    /// Explicitly marked junk.
    MarkedJunk,
}

impl Engagement {
    /// Counter fields this event bumps.
    fn fields(self) -> &'static [&'static [u8]] {
        match self {
            Engagement::Read { fast: true } => &[b"read_count", b"read_fast_count"],
            Engagement::Read { fast: false } => &[b"read_count"],
            Engagement::ArchivedUnread => &[b"archived_unread_count"],
            Engagement::Starred => &[b"starred_count"],
            Engagement::MarkedJunk => &[b"marked_junk_count"],
        }
    }
}

/// Record one engagement event against a sender's relationship record.
///
/// Counters, not an event log: the learner needs rates per sender
/// (opened 9 of 10, archived unread 8 of 10), and a rate is all a raw
/// log would be reduced to anyway. HINCRBY keeps concurrent marks from
/// losing increments.
///
/// Best-effort by contract — the caller is servicing a user action that
/// must not fail because a derived counter could not be written.
pub fn record_engagement(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    event: Engagement,
) -> std::io::Result<()> {
    if is_self(user, email) {
        return Ok(());
    }
    let key = contact_key(user, email);
    conn.pipeline(|p| {
        for f in event.fields() {
            p.cmd(&[b"HINCRBY", key.as_bytes(), f, b"1"]);
        }
    })?;
    Ok(())
}

/// Read one sender's scoring record.
///
/// A sender with no record yields the all-false / zero default, which is
/// the correct reading: no relationship is known yet.
pub fn scoring_for(conn: &mut kevy_client::Connection, user: &str, email: &str) -> ContactScoring {
    let key = contact_key(user, email);
    let received = hget_u32(conn, &key, "received_count");
    let sent = hget_u32(conn, &key, "sent_count");
    ContactScoring {
        // Mutual means traffic has flowed both ways.
        is_mutual: received > 0 && sent > 0,
        is_mailing_list: hget_bool(conn, &key, "is_mailing_list"),
        is_vip: hget_bool(conn, &key, "is_vip"),
        is_blocked: hget_bool(conn, &key, "is_blocked"),
        importance_bias: hget_str(conn, &key, "importance_bias")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        received_count: received,
        sent_count: sent,
    }
}

/// True when the user has ever sent to this address — the "this is a
/// reply to something I started" relationship signal.
pub fn has_sent_to_addr(conn: &mut kevy_client::Connection, user: &str, email: &str) -> bool {
    hget_u32(conn, &contact_key(user, email), "sent_count") > 0
}

#[cfg(test)]
mod tests {
    use super::is_self;

    #[test]
    fn self_send_is_detected_case_and_space_insensitively() {
        assert!(is_self("lihao@golia.jp", "lihao@golia.jp"));
        assert!(is_self("lihao@golia.jp", "LIHAO@GOLIA.JP"));
        assert!(is_self("lihao@golia.jp", "  lihao@golia.jp "));
    }

    #[test]
    fn a_different_correspondent_is_not_self() {
        assert!(!is_self("lihao@golia.jp", "alice@x.com"));
        // a substring / same local-part on another domain is not self
        assert!(!is_self("lihao@golia.jp", "lihao@other.jp"));
    }
}
