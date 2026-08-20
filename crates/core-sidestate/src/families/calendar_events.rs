//! One shape for a stored calendar event, because there were three.
//!
//! ```text
//!   calendar_events:{user}         zset  score = dtstart epoch, member = uid
//!   calendar_event:{user}:{uid}    hash  this row, one field per column
//! ```
//!
//! Until 2026-08-20 the feed sync wrote the whole row into a single
//! `json` field, the conflicts reader looked for flat fields it never
//! wrote, and CalDAV `PUT` stored raw iCalendar under a fourth key
//! entirely. Each side was self-consistent; a subscribed calendar's
//! events simply never appeared in a conflict check, and nothing
//! errored, because a hash missing every field a reader asks for reads
//! as an empty row rather than as an absence.
//!
//! So the columns live here, with the writer and the reader beside each
//! other, and every producer goes through them.

use serde::{Deserialize, Serialize};

/// An event as stored, and as the conflicts reader wants it.
///
/// Times are RFC 3339 instants — resolved before they get here, because
/// a wall-clock plus a zone name is not a moment and two rows in that
/// state cannot be compared for overlap at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    /// iCalendar `UID` — the event's identity across updates.
    pub uid: String,
    /// `SUMMARY`.
    #[serde(default)]
    pub summary: String,
    /// Start, RFC 3339. Absent for an all-day event, which has none.
    #[serde(default)]
    pub dtstart: Option<String>,
    /// End, RFC 3339.
    #[serde(default)]
    pub dtend: Option<String>,
    /// Organiser's address.
    #[serde(default)]
    pub organizer: Option<String>,
    /// `CONFIRMED` / `TENTATIVE` / `CANCELLED`.
    #[serde(default)]
    pub status: Option<String>,
    /// Where it came from — `feed:{id}`, `mail:{message_id}`, `caldav`.
    ///
    /// So unsubscribing can remove a feed's events, and so an event a
    /// person made by hand is never mistaken for one that syncs.
    #[serde(default)]
    pub source: String,
    /// iCalendar `SEQUENCE`. A lower one never overwrites a higher: an
    /// organiser re-sends a meeting on every change, and old copies of
    /// the mail are still in the mailbox to be re-read.
    #[serde(default)]
    pub sequence: i32,
}

/// The hash key for one event.
pub fn event_key(user: &str, uid: &str) -> String {
    format!("calendar_event:{user}:{uid}")
}

/// The per-user index, scored by start time.
pub fn index_key(user: &str) -> String {
    format!("calendar_events:{user}")
}

/// The field/value pairs to write, in the order a hash writer wants
/// them.
pub fn fields(ev: &StoredEvent) -> Vec<(&'static str, String)> {
    vec![
        ("summary", ev.summary.clone()),
        ("dtstart", ev.dtstart.clone().unwrap_or_default()),
        ("dtend", ev.dtend.clone().unwrap_or_default()),
        ("organizer", ev.organizer.clone().unwrap_or_default()),
        ("status", ev.status.clone().unwrap_or_default()),
        ("source", ev.source.clone()),
        ("sequence", ev.sequence.to_string()),
    ]
}

/// Read a row back from a flat `HGETALL` result.
///
/// Understands the legacy single-`json` shape the feed sync wrote before
/// this module existed, so events already on disk are readable rather
/// than blank. Delete that arm once no `json` field remains — a census
/// of `calendar_event:*` says when.
pub fn from_flat(uid: &str, flat: &[Vec<u8>]) -> StoredEvent {
    let mut ev = StoredEvent {
        uid: uid.to_string(),
        ..Default::default()
    };
    let mut i = 0;
    while i + 1 < flat.len() {
        let k = String::from_utf8_lossy(&flat[i]);
        let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
        match k.as_ref() {
            "summary" => ev.summary = v,
            "dtstart" => ev.dtstart = Some(v).filter(|s| !s.is_empty()),
            "dtend" => ev.dtend = Some(v).filter(|s| !s.is_empty()),
            "organizer" => ev.organizer = Some(v).filter(|s| !s.is_empty()),
            "status" => ev.status = Some(v).filter(|s| !s.is_empty()),
            "source" => ev.source = v,
            "sequence" => ev.sequence = v.parse().unwrap_or(0),
            "json" => {
                if let Ok(legacy) = serde_json::from_str::<StoredEvent>(&v) {
                    let seq = ev.sequence;
                    ev = StoredEvent {
                        uid: uid.to_string(),
                        ..legacy
                    };
                    ev.sequence = ev.sequence.max(seq);
                }
            }
            _ => {}
        }
        i += 2;
    }
    ev
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(pairs: &[(&str, &str)]) -> Vec<Vec<u8>> {
        pairs
            .iter()
            .flat_map(|(k, v)| [k.as_bytes().to_vec(), v.as_bytes().to_vec()])
            .collect()
    }

    #[test]
    fn a_row_survives_the_round_trip() {
        let ev = StoredEvent {
            uid: "u1".into(),
            summary: "Product sync".into(),
            dtstart: Some("2026-08-20T23:00:00+00:00".into()),
            dtend: Some("2026-08-20T23:50:00+00:00".into()),
            organizer: Some("chair@example.com".into()),
            status: Some("CONFIRMED".into()),
            source: "mail:m1@example.com".into(),
            sequence: 9,
        };
        let written = fields(&ev);
        let pairs: Vec<(&str, &str)> = written.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(from_flat("u1", &flat(&pairs)), ev);
    }

    /// The feed sync's rows, written before this module existed, must
    /// still read — otherwise the fix for "conflicts never sees feed
    /// events" replaces blank rows with different blank rows.
    #[test]
    fn the_legacy_json_shape_still_reads() {
        let legacy = r#"{"uid":"u2","summary":"Standup","dtstart":"2026-08-20T09:00:00+00:00","dtend":null,"organizer":null,"status":null,"source":"feed:f1"}"#;
        let ev = from_flat("u2", &flat(&[("json", legacy)]));
        assert_eq!(ev.summary, "Standup");
        assert_eq!(ev.dtstart.as_deref(), Some("2026-08-20T09:00:00+00:00"));
        assert_eq!(ev.source, "feed:f1");
    }

    /// A hash with nothing a reader recognises is an empty row, and an
    /// empty row is what a missing key looks like too. The uid is the
    /// one thing that always comes from the caller, so it is the only
    /// field that can tell them apart.
    #[test]
    fn an_unreadable_row_still_names_itself() {
        let ev = from_flat("u3", &flat(&[("something-else", "x")]));
        assert_eq!(ev.uid, "u3");
        assert!(ev.summary.is_empty());
    }
}
