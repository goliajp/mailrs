//! Which part of the sectioned inbox a conversation belongs to, for one
//! reader: waiting on the other side, a feed, a first contact, one for them
//! to answer, or the rest.
//!
//! Decided from the conversation's last messages, never stored as a fact:
//! the facts are recorded per message as it is delivered ([`MessageFacts`])
//! and the section is derived from them. Both cores call [`section_of`];
//! the SQL core states the same rules as a `CASE`, and a test holds the two
//! to the same answers.

/// What delivery records about one message, for its reader.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageFacts {
    /// The reader's own address is in `To:` — not only `Cc:`, `Bcc:`, an
    /// alias or a group.
    pub direct: bool,
    /// Written by a person: not an automated sender, not a mailing list,
    /// not filed with notifications or promotions.
    pub from_person: bool,
    /// Nothing had passed between the reader and this sender when it
    /// arrived: nothing received from them, nothing sent to them, and they
    /// had not been accepted.
    pub first_contact: bool,
    /// For the reader's own message: at least one recipient is someone
    /// else. A note to self waits on nobody.
    pub to_others: bool,
    /// A calendar invitation: it is answered with the card's buttons, not
    /// a reply.
    pub invite: bool,
    /// The reader and this sender write to each other: by the counts when
    /// it arrived, [`mutual`] of what the reader sent them and what they
    /// sent the reader.
    pub mutual: bool,
}

/// Whether mail goes both ways between the reader and a sender: the reader
/// has written to them at least once, and at least a fifth as often as
/// they have written to the reader. A support address that sends a hundred
/// notices for every three letters it gets is not a correspondent.
pub fn mutual(sent: u64, received: u64) -> bool {
    sent >= 1 && sent.saturating_mul(5) >= received
}

/// The sections, as stored and sent on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The reader wrote last and a person has not answered since.
    Awaiting,
    /// The last message from someone else came from a machine or a list.
    Feed,
    /// The last message is from someone the reader has never dealt with.
    New,
    /// The last message is from a person, addressed to the reader, not an
    /// invitation, and from someone the reader writes to or in a
    /// conversation the reader has written in.
    Reply,
    /// Copied, group mail, notes to self.
    Other,
}

impl Section {
    /// Every section, in the order the sectioned inbox shows them.
    pub const ALL: [Section; 5] = [
        Section::Awaiting,
        Section::Feed,
        Section::New,
        Section::Reply,
        Section::Other,
    ];

    /// The name stored on a row and sent on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Section::Awaiting => "awaiting",
            Section::Feed => "feed",
            Section::New => "new",
            Section::Reply => "reply",
            Section::Other => "other",
        }
    }

    /// The section a stored or requested name stands for, case-insensitively.
    pub fn named(name: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// A conversation's last messages, as the section is decided from them.
/// Times are epoch seconds, 0 when there is no such message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Latest {
    /// The reader's own last message.
    pub own_at: i64,
    /// Whether that message went to anyone besides the reader.
    pub own_to_others: bool,
    /// The last message from a person other than the reader.
    pub person_at: i64,
    /// The last message from anyone else.
    pub other_at: i64,
    /// What delivery recorded about that message.
    pub other: MessageFacts,
    /// The sender of that last message has been accepted since it arrived.
    pub other_accepted: bool,
}

impl Latest {
    /// Fold one message in: a time later than, or equal to, the one held on
    /// its side replaces it, so the same messages in any order end in the
    /// same value. The kevy row keeps its copy of these by the same rule.
    pub fn fold(&mut self, at: i64, is_own: bool, facts: MessageFacts) {
        if is_own {
            if at >= self.own_at {
                self.own_at = at;
                self.own_to_others = facts.to_others;
            }
            return;
        }
        if at >= self.other_at {
            self.other_at = at;
            self.other = facts;
            self.other_accepted = false;
        }
        if facts.from_person && at >= self.person_at {
            self.person_at = at;
        }
    }
}

/// The section, by the rules in order; the first that holds wins.
///
/// A machine's answer does not count as a reply: following up is waiting
/// for a person, so only `person_at` ends the wait.
pub fn section_of(l: &Latest) -> Section {
    if l.own_at > 0 && l.own_at > l.person_at && l.own_to_others {
        return Section::Awaiting;
    }
    if l.other_at == 0 {
        return Section::Other;
    }
    section_of_inbound(l.other, l.other_accepted, l.own_at > 0)
}

/// The section the last message from someone else puts a conversation in,
/// when the reader is not waiting: `accepted` is whether its sender has
/// been accepted since it arrived, `reader_wrote` whether the reader has
/// written in the conversation.
///
/// Reply is mail from a person, to the reader, that wants a written
/// answer: not an invitation, and from someone the reader writes to or in
/// a conversation the reader has written in. Without the last two, Reply
/// fills with alerts, card statements and verification codes from
/// addresses the reader has never answered.
pub fn section_of_inbound(other: MessageFacts, accepted: bool, reader_wrote: bool) -> Section {
    if !other.from_person {
        return Section::Feed;
    }
    if other.first_contact && !accepted && !reader_wrote {
        return Section::New;
    }
    if other.direct && !other.invite && (reader_wrote || other.mutual) {
        return Section::Reply;
    }
    Section::Other
}

/// Whether a message just delivered puts its conversation in Reply, by the
/// same rule as [`section_of`]: what the push policy rings for. Delivery
/// read `first_contact` with any acceptance already counted.
pub fn owed_a_reply(facts: MessageFacts, reader_wrote: bool) -> bool {
    section_of_inbound(facts, false, reader_wrote) == Section::Reply
}

/// When the conversation started waiting, for the follow-up index: the
/// reader's last message while it is [`Section::Awaiting`], else 0.
pub fn awaiting_since(l: &Latest) -> i64 {
    match section_of(l) {
        Section::Awaiting => l.own_at,
        _ => 0,
    }
}

#[path = "section_fixtures.rs"]
mod fixtures;
pub use fixtures::fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_gets_its_section() {
        let wrong: Vec<String> = fixtures()
            .into_iter()
            .filter(|(_, latest, want)| section_of(latest) != *want)
            .map(|(name, latest, want)| format!("{name}: {:?}, want {want:?}", section_of(&latest)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn the_wait_starts_at_my_last_message() {
        for (name, latest, want) in fixtures() {
            let since = awaiting_since(&latest);
            match want {
                Section::Awaiting => assert_eq!(since, latest.own_at, "{name}"),
                _ => assert_eq!(since, 0, "{name}"),
            }
        }
    }

    /// A message on its own decides Reply as the conversation it is the
    /// last of: every fixture the reader is not waiting on agrees.
    #[test]
    fn a_delivered_message_is_owed_a_reply_as_its_conversation_is() {
        for (name, l, want) in fixtures() {
            if l.other_at == 0 || want == Section::Awaiting || l.other_accepted {
                continue;
            }
            assert_eq!(
                owed_a_reply(l.other, l.own_at > 0),
                want == Section::Reply,
                "{name}"
            );
        }
    }

    #[test]
    fn mail_goes_both_ways_from_a_fifth() {
        assert!(mutual(1, 5));
        assert!(!mutual(1, 6));
        assert!(mutual(69, 69));
        assert!(mutual(30, 31));
        assert!(!mutual(3, 132));
        assert!(!mutual(0, 0), "nothing sent is not mutual");
        assert!(mutual(4, 0));
    }

    #[test]
    fn names_round_trip() {
        for s in Section::ALL {
            assert_eq!(Section::named(s.name()), Some(s));
        }
        assert_eq!(Section::named("REPLY"), Some(Section::Reply));
        assert_eq!(Section::named("important"), None);
    }
}
