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
    /// The last message is from a person and addressed to the reader.
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
    if !l.other.from_person {
        return Section::Feed;
    }
    if l.other.first_contact && !l.other_accepted && l.own_at == 0 {
        return Section::New;
    }
    if l.other.direct {
        return Section::Reply;
    }
    Section::Other
}

/// When the conversation started waiting, for the follow-up index: the
/// reader's last message while it is [`Section::Awaiting`], else 0.
pub fn awaiting_since(l: &Latest) -> i64 {
    match section_of(l) {
        Section::Awaiting => l.own_at,
        _ => 0,
    }
}

/// The fixtures both cores are held to: each with the section it must get.
/// The SQL core's `CASE` is tested against these rows.
pub fn fixtures() -> Vec<(&'static str, Latest, Section)> {
    let person = MessageFacts {
        direct: true,
        from_person: true,
        ..MessageFacts::default()
    };
    let mut out = vec![
        (
            "a person wrote to me",
            Latest {
                other_at: 10,
                person_at: 10,
                other: person,
                ..Latest::default()
            },
            Section::Reply,
        ),
        (
            "I answered, nobody since",
            Latest {
                own_at: 20,
                own_to_others: true,
                other_at: 10,
                person_at: 10,
                other: person,
                ..Latest::default()
            },
            Section::Awaiting,
        ),
        (
            "I wrote and a person answered",
            Latest {
                own_at: 20,
                own_to_others: true,
                other_at: 30,
                person_at: 30,
                other: person,
                ..Latest::default()
            },
            Section::Reply,
        ),
        (
            "a ticket robot answered my mail",
            Latest {
                own_at: 20,
                own_to_others: true,
                other_at: 30,
                person_at: 0,
                other: MessageFacts::default(),
                ..Latest::default()
            },
            Section::Awaiting,
        ),
        (
            "sent and never answered",
            Latest {
                own_at: 20,
                own_to_others: true,
                ..Latest::default()
            },
            Section::Awaiting,
        ),
        (
            "a note to self",
            Latest {
                own_at: 20,
                own_to_others: false,
                ..Latest::default()
            },
            Section::Other,
        ),
        (
            "a newsletter",
            Latest {
                other_at: 10,
                other: MessageFacts {
                    direct: true,
                    ..MessageFacts::default()
                },
                ..Latest::default()
            },
            Section::Feed,
        ),
        (
            "a stranger wrote",
            Latest {
                other_at: 10,
                person_at: 10,
                other: MessageFacts {
                    first_contact: true,
                    ..person
                },
                ..Latest::default()
            },
            Section::New,
        ),
        (
            "a stranger, accepted",
            Latest {
                other_at: 10,
                person_at: 10,
                other: MessageFacts {
                    first_contact: true,
                    ..person
                },
                other_accepted: true,
                ..Latest::default()
            },
            Section::Reply,
        ),
        (
            "copied on a thread",
            Latest {
                other_at: 10,
                person_at: 10,
                other: MessageFacts {
                    direct: false,
                    ..person
                },
                ..Latest::default()
            },
            Section::Other,
        ),
    ];
    // a stranger I have already answered is no longer new
    out.push((
        "a stranger I answered, who wrote again",
        Latest {
            own_at: 15,
            own_to_others: true,
            other_at: 20,
            person_at: 20,
            other: MessageFacts {
                first_contact: true,
                ..person
            },
            ..Latest::default()
        },
        Section::Reply,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_gets_its_section() {
        for (name, latest, want) in fixtures() {
            assert_eq!(section_of(&latest), want, "{name}");
        }
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

    #[test]
    fn names_round_trip() {
        for s in Section::ALL {
            assert_eq!(Section::named(s.name()), Some(s));
        }
        assert_eq!(Section::named("REPLY"), Some(Section::Reply));
        assert_eq!(Section::named("important"), None);
    }
}
