//! One-shot move of what the old CalDAV / CardDAV handlers stored into the
//! rows everything else reads.
//!
//! ```text
//!   caldav:{user}:events:{calendar}    hash  uid → iCalendar   → calendar_events
//!   carddav:{user}:contacts:{book}     hash  uid → vCard       → contact_cards
//! ```
//!
//! Those handlers wrote raw text under keys no other reader looked at, so a
//! phone's events never reached the web's conflicts check. Every calendar
//! and book of the old layout lands in the user's one default calendar or
//! address book.
//!
//! A row already present under the unified key is left alone: it is either
//! this migration's own earlier write (`already`) or something newer from
//! another writer (`kept_existing`). So a second run writes nothing, and
//! the counts of what was walked say whether that is because the work is
//! done or because there was nothing to read. Old keys are deleted only
//! when asked, and only those whose every entry is now represented.

use std::io;

use kevy_client::Connection;
use serde::Serialize;

use super::{calendar_events, contact_cards};

/// What one run saw and did, per kind.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct KindReport {
    /// Old keys found.
    pub keys: u64,
    /// Entries read out of them.
    pub walked: u64,
    /// Entries copied to the unified key.
    pub written: u64,
    /// Entries whose unified copy is identical — an earlier run's work.
    pub already: u64,
    /// Entries whose uid already holds something else; left as it is.
    pub kept_existing: u64,
    /// Entries that do not parse, with why; left in the old key.
    pub unparseable: u64,
    /// Old keys removed (only with `delete_old`).
    pub old_keys_deleted: u64,
}

/// Both kinds, and whether deletion was asked for.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// `caldav:*:events:*`.
    pub events: KindReport,
    /// `carddav:*:contacts:*`.
    pub contacts: KindReport,
    /// Whether old keys were eligible for deletion.
    pub delete_old: bool,
    /// Up to twenty `key uid: reason` lines for unparseable entries.
    pub unparseable_samples: Vec<String>,
}

const SAMPLE_LIMIT: usize = 20;

/// The user an old key belongs to: `{prefix}{user}{infix}{name}`.
fn user_of(key: &str, prefix: &str, infix: &str) -> Option<String> {
    let rest = key.strip_prefix(prefix)?;
    let (user, _) = rest.split_once(infix)?;
    Some(user.to_string())
}

fn scan_all(conn: &mut Connection, pattern: &str) -> io::Result<Vec<String>> {
    let mut keys = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, batch) = conn.scan(cursor, Some(pattern.as_bytes()), Some(500))?;
        keys.extend(
            batch
                .into_iter()
                .map(|k| String::from_utf8_lossy(&k).into_owned()),
        );
        if next == 0 {
            break;
        }
        cursor = next;
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

enum Outcome {
    Written,
    Already,
    KeptExisting,
    Unparseable(String),
}

fn move_event(conn: &mut Connection, user: &str, uid: &str, text: &str) -> io::Result<Outcome> {
    let row = match calendar_events::from_ics(uid, text, "caldav") {
        Ok(r) => r,
        Err(why) => return Ok(Outcome::Unparseable(why)),
    };
    match calendar_events::get(conn, user, uid)? {
        Some(existing) if existing.ics.as_deref() == Some(text) => Ok(Outcome::Already),
        Some(_) => Ok(Outcome::KeptExisting),
        None => calendar_events::put(conn, user, &row).map(|_| Outcome::Written),
    }
}

fn move_card(conn: &mut Connection, user: &str, uid: &str, text: &str) -> io::Result<Outcome> {
    if let Err(why) = contact_cards::parse_card(text) {
        return Ok(Outcome::Unparseable(why));
    }
    match contact_cards::get(conn, user, uid)? {
        Some(existing) if existing == text => Ok(Outcome::Already),
        Some(_) => Ok(Outcome::KeptExisting),
        None => contact_cards::put(conn, user, uid, text).map(|_| Outcome::Written),
    }
}

type Mover = fn(&mut Connection, &str, &str, &str) -> io::Result<Outcome>;

fn migrate_kind(
    conn: &mut Connection,
    (prefix, infix): (&str, &str),
    mover: Mover,
    delete_old: bool,
    samples: &mut Vec<String>,
) -> io::Result<KindReport> {
    let mut r = KindReport::default();
    for key in scan_all(conn, &format!("{prefix}*{infix}*"))? {
        let Some(user) = user_of(&key, prefix, infix) else {
            continue;
        };
        r.keys += 1;
        let flat = conn.hgetall(key.as_bytes())?;
        let mut clean = true;
        for pair in flat.chunks(2) {
            let [uid, text] = pair else { continue };
            let (uid, text) = (String::from_utf8_lossy(uid), String::from_utf8_lossy(text));
            r.walked += 1;
            match mover(conn, &user, &uid, &text)? {
                Outcome::Written => r.written += 1,
                Outcome::Already => r.already += 1,
                Outcome::KeptExisting => r.kept_existing += 1,
                Outcome::Unparseable(why) => {
                    r.unparseable += 1;
                    clean = false;
                    if samples.len() < SAMPLE_LIMIT {
                        samples.push(format!("{key} {uid}: {why}"));
                    }
                }
            }
        }
        if delete_old && clean {
            r.old_keys_deleted += conn.del(&[key.as_bytes()])? as u64;
        }
    }
    Ok(r)
}

/// Copy every old-layout event and card into the unified keys; with
/// `delete_old`, then remove the old keys that copied cleanly.
pub fn migrate(conn: &mut Connection, delete_old: bool) -> io::Result<Report> {
    let mut samples = Vec::new();
    let events = migrate_kind(
        conn,
        ("caldav:", ":events:"),
        move_event,
        delete_old,
        &mut samples,
    )?;
    let contacts = migrate_kind(
        conn,
        ("carddav:", ":contacts:"),
        move_card,
        delete_old,
        &mut samples,
    )?;
    Ok(Report {
        events,
        contacts,
        delete_old,
        unparseable_samples: samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_is_read_out_of_an_old_key() {
        assert_eq!(
            user_of("caldav:a@b.example:events:default", "caldav:", ":events:").as_deref(),
            Some("a@b.example")
        );
        assert_eq!(
            user_of("caldav:a@b.example:calendars", "caldav:", ":events:"),
            None
        );
    }
}
