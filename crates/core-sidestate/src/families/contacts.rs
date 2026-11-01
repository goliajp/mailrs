//! Per-sender relationship records on the network kevy, which both cores
//! write and importance scoring reads:
//!   `mailrs:contact:{user}:{email}`   hash of scoring fields

use mailrs_core_api::method::contact::ContactScoring;

pub fn contact_key(user: &str, email: &str) -> String {
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
/// Whether anything has passed between `user` and `email`: a message
/// received from them, one sent to them, or an acceptance. `false` for a
/// sender never seen; read before the arriving message is recorded, so a
/// message is never evidence of its own history.
pub fn known(conn: &mut kevy_client::Connection, user: &str, email: &str) -> bool {
    let key = contact_key(user, email);
    hget_u32(conn, &key, "received_count") > 0
        || hget_u32(conn, &key, "sent_count") > 0
        || hget_str(conn, &key, "accepted_at").is_some()
}

/// Record that `user` accepted `email` as a sender, at `now`. A fact of its
/// own: what delivery recorded about earlier messages stays as it was.
pub fn accept(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    now: i64,
) -> std::io::Result<()> {
    conn.hset(
        contact_key(user, email).as_bytes(),
        &[(b"accepted_at" as &[u8], now.to_string().as_bytes())],
    )?;
    Ok(())
}

/// Whether `user` has accepted `email` as a sender.
pub fn accepted(conn: &mut kevy_client::Connection, user: &str, email: &str) -> bool {
    hget_str(conn, &contact_key(user, email), "accepted_at").is_some()
}

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

/// What ranks one address in recipient autocomplete.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Standing {
    pub sent: u32,
    pub received: u32,
    /// An automated sender or a mailing list: ranked after every person.
    pub automated: bool,
    /// How recently `user` last wrote to it, as a rank: higher is more
    /// recent, 0 is never.
    pub last_sent: f64,
}

impl Standing {
    /// A message written to someone says more than one received from them.
    fn weight(&self) -> u64 {
        u64::from(self.sent) * 3 + u64::from(self.received)
    }
}

/// [`Standing`] for each of `emails`, in order: one read of the send-time
/// set, then one per address. Not a pipeline, which the embedded store
/// does not serve.
pub fn standings(
    conn: &mut kevy_client::Connection,
    user: &str,
    emails: &[String],
) -> std::io::Result<Vec<Standing>> {
    // members come back oldest first, so a position orders by recency
    let by_time = conn.zrange(
        super::contact_cards::autocomplete_ts_key(user).as_bytes(),
        0,
        -1,
    )?;
    let order: std::collections::HashMap<Vec<u8>, usize> = by_time
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, i + 1))
        .collect();
    let mut out = Vec::with_capacity(emails.len());
    for email in emails {
        let flat = conn.hgetall(contact_key(user, email).as_bytes())?;
        let mut st = Standing {
            last_sent: order.get(email.as_bytes()).copied().unwrap_or(0) as f64,
            ..Standing::default()
        };
        for pair in flat.chunks_exact(2) {
            let value = String::from_utf8_lossy(&pair[1]);
            match pair[0].as_slice() {
                b"sent_count" => st.sent = value.parse().unwrap_or(0),
                b"received_count" => st.received = value.parse().unwrap_or(0),
                b"is_automated" | b"is_mailing_list" => st.automated |= value == "1",
                _ => {}
            }
        }
        out.push(st);
    }
    Ok(out)
}

/// People before automated senders, then the most written to, then the
/// most recently written to, then by what is shown.
pub fn rank(entries: &mut [(String, Standing)]) {
    entries.sort_by(|(a_name, a), (b_name, b)| {
        a.automated
            .cmp(&b.automated)
            .then_with(|| b.weight().cmp(&a.weight()))
            .then_with(|| b.last_sent.total_cmp(&a.last_sent))
            .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
    });
}

/// The set listing `user`'s important contacts, beside each contact's
/// `is_vip` field (which scoring reads); [`set_vip`] writes both.
pub fn vips_key(user: &str) -> String {
    format!("mailrs:contact_vips:{}", user.to_lowercase())
}

/// Mark `email` as one of `user`'s important contacts, or not. Important
/// contacts' mail is pushed and ranks higher.
pub fn set_vip(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    vip: bool,
) -> std::io::Result<()> {
    let email = email.trim().to_ascii_lowercase();
    let key = contact_key(user, &email);
    match vip {
        true => {
            conn.hset(key.as_bytes(), &[(b"is_vip" as &[u8], b"1" as &[u8])])?;
            conn.sadd(vips_key(user).as_bytes(), &[email.as_bytes()])?;
        }
        false => {
            conn.hset(key.as_bytes(), &[(b"is_vip" as &[u8], b"0" as &[u8])])?;
            conn.srem(vips_key(user).as_bytes(), &[email.as_bytes()])?;
        }
    }
    Ok(())
}

/// Whether `email` is one of `user`'s important contacts.
pub fn is_vip(conn: &mut kevy_client::Connection, user: &str, email: &str) -> bool {
    hget_bool(
        conn,
        &contact_key(user, &email.trim().to_ascii_lowercase()),
        "is_vip",
    )
}

/// `user`'s important contacts, sorted.
pub fn vips(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = conn
        .smembers(vips_key(user).as_bytes())?
        .into_iter()
        .map(|m| String::from_utf8_lossy(&m).into_owned())
        .collect();
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{contact_key, is_self};

    #[test]
    fn the_scoring_key_keeps_its_prod_spelling() {
        assert_eq!(
            contact_key("a@x.jp", "b@y.jp"),
            "mailrs:contact:a@x.jp:b@y.jp"
        );
    }

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

#[cfg(test)]
mod rank_tests {
    use super::*;

    fn s(sent: u32, received: u32, automated: bool, last_sent: f64) -> Standing {
        Standing {
            sent,
            received,
            automated,
            last_sent,
        }
    }

    /// Read off what the writers store: the contact hash and the send-time set.
    #[test]
    fn standings_read_the_stored_counts_and_recency() {
        let mut c = kevy_client::Connection::connect("mem://contacts-standings").unwrap();
        let u = "me@x";
        c.hset(
            contact_key(u, "c@x").as_bytes(),
            &[
                (b"sent_count" as &[u8], b"5" as &[u8]),
                (b"received_count", b"2"),
            ],
        )
        .unwrap();
        c.hset(
            contact_key(u, "bot@x").as_bytes(),
            &[
                (b"received_count" as &[u8], b"40" as &[u8]),
                (b"is_automated", b"1"),
            ],
        )
        .unwrap();
        let ts = super::super::contact_cards::autocomplete_ts_key(u);
        c.zadd(ts.as_bytes(), &[(100.0, b"c@x" as &[u8]), (50.0, b"old@x")])
            .unwrap();
        let got = standings(
            &mut c,
            u,
            &["c@x".to_string(), "bot@x".to_string(), "none@x".to_string()],
        )
        .unwrap();
        assert_eq!(got[0], s(5, 2, false, 2.0));
        assert_eq!(got[1], s(0, 40, true, 0.0));
        assert_eq!(got[2], Standing::default());
    }

    #[test]
    fn the_most_written_to_come_first_and_robots_last() {
        let mut v = vec![
            ("Alpha <a@x>".to_string(), s(0, 1, false, 0.0)),
            ("Bot <noreply@x>".to_string(), s(0, 90, true, 0.0)),
            ("Carol <c@x>".to_string(), s(5, 0, false, 10.0)),
            ("Dan <d@x>".to_string(), s(0, 9, false, 0.0)),
            ("Erin <e@x>".to_string(), s(5, 0, false, 20.0)),
        ];
        rank(&mut v);
        let order: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "Erin <e@x>",
                "Carol <c@x>",
                "Dan <d@x>",
                "Alpha <a@x>",
                "Bot <noreply@x>"
            ]
        );
    }

    #[test]
    fn an_important_contact_is_listed_and_scored() {
        let mut c = kevy_client::Connection::connect("mem://contacts-vip").unwrap();
        set_vip(&mut c, "me@x", " Boss@X.example ", true).unwrap();
        assert!(is_vip(&mut c, "me@x", "boss@x.example"));
        assert!(scoring_for(&mut c, "me@x", "boss@x.example").is_vip);
        assert_eq!(vips(&mut c, "me@x").unwrap(), vec!["boss@x.example"]);
        set_vip(&mut c, "me@x", "boss@x.example", false).unwrap();
        assert!(!is_vip(&mut c, "me@x", "boss@x.example"));
        assert!(vips(&mut c, "me@x").unwrap().is_empty());
    }
}
