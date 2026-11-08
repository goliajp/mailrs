//! Reading the section's inputs off a membership row, and deciding from
//! them.

use mailrs_mailbox::section::{Latest, MessageFacts, awaiting_since, section_of};

use super::{
    IN_ACCEPTED, IN_AT, IN_DIRECT, IN_FIRST, IN_FROM, IN_INVITE, IN_MUTUAL, IN_PERSON, OWN_AT,
    OWN_TO_OTHERS, PERSON_AT, SectionArrival,
};

pub(super) type Fields = std::collections::HashMap<Vec<u8>, Vec<u8>>;

pub(super) fn num(fields: &Fields, f: &[u8]) -> i64 {
    fields
        .get(f)
        .and_then(|v| std::str::from_utf8(v).ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub(super) fn on(fields: &Fields, f: &[u8]) -> bool {
    fields.get(f).map(Vec::as_slice) == Some(b"1")
}

pub(super) fn text(fields: &Fields, f: &[u8]) -> String {
    String::from_utf8_lossy(fields.get(f).map_or(&[][..], Vec::as_slice)).into_owned()
}

/// The section and `awaiting_since` a row with these inputs gets: none in
/// Junk, where the sectioned inbox does not reach.
pub(super) fn decided(fields: &Fields, latest: &Latest) -> (&'static str, String) {
    let junk = fields.get(b"bucket".as_slice()).map(Vec::as_slice) == Some(b"junk");
    match junk {
        true => ("", "0".to_string()),
        false => (
            section_of(latest).name(),
            awaiting_since(latest).to_string(),
        ),
    }
}

/// The decision's inputs, as the row carries them. A row recorded before
/// `invite` and `mutual` existed reads both as false.
pub(super) fn latest_from(fields: &Fields) -> Latest {
    Latest {
        own_at: num(fields, OWN_AT),
        own_to_others: on(fields, OWN_TO_OTHERS),
        person_at: num(fields, PERSON_AT),
        other_at: num(fields, IN_AT),
        other: MessageFacts {
            direct: on(fields, IN_DIRECT),
            from_person: on(fields, IN_PERSON),
            first_contact: on(fields, IN_FIRST),
            to_others: false,
            invite: on(fields, IN_INVITE),
            mutual: on(fields, IN_MUTUAL),
        },
        other_accepted: on(fields, IN_ACCEPTED),
    }
}

/// The inputs `arrivals` fold to, by the rule [`Latest::fold`] shares with
/// [`KevyMailboxStore::record_section_arrival`], and the newest inbound
/// sender (the row's, when nothing inbound is among them). What delivery
/// read about the newest inbound message stands while it still is that,
/// except that a first contact the row did not record is taken from the
/// arrival: old mail was decided by a backfill that could not tell, and an
/// arrival says first contact only from history that shows it.
pub(super) fn refolded(old: &Fields, arrivals: &[SectionArrival<'_>]) -> (Latest, String) {
    let mut latest = Latest::default();
    let mut from = None;
    for a in arrivals {
        if !a.is_own && a.at >= latest.other_at {
            from = Some(a.from_addr);
        }
        latest.fold(a.at, a.is_own, a.facts);
    }
    let old_from = text(old, IN_FROM);
    let from = from.map_or_else(|| old_from.clone(), str::to_string);
    let same_latest =
        old.contains_key(IN_AT) && num(old, IN_AT) == latest.other_at && from == old_from;
    if same_latest {
        latest.other.first_contact |= on(old, IN_FIRST);
        latest.other_accepted = on(old, IN_ACCEPTED);
        if old.contains_key(IN_MUTUAL) {
            latest.other.mutual = on(old, IN_MUTUAL);
        }
    }
    (latest, from)
}
