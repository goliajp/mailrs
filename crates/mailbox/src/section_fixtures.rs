//! The fixtures both cores are held to, with the section each must get.

use super::{Latest, MessageFacts, Section, mutual};

/// The fixtures both cores are held to: each with the section it must get.
/// The SQL core's `CASE` is tested against these rows.
pub fn fixtures() -> Vec<(&'static str, Latest, Section)> {
    let person = MessageFacts {
        direct: true,
        from_person: true,
        mutual: true,
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
            // accepted is no longer new, but nothing has gone back to them
            "a stranger, accepted",
            Latest {
                other_at: 10,
                person_at: 10,
                other: MessageFacts {
                    first_contact: true,
                    mutual: false,
                    ..person
                },
                other_accepted: true,
                ..Latest::default()
            },
            Section::Other,
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
                mutual: false,
                ..person
            },
            ..Latest::default()
        },
        Section::Reply,
    ));
    out.extend(stricter_reply(person));
    out
}

/// Who is owed a reply: someone the reader writes to, or a conversation
/// the reader has taken part in; never a calendar invitation.
fn stricter_reply(person: MessageFacts) -> Vec<(&'static str, Latest, Section)> {
    let from = |other: MessageFacts| Latest {
        other_at: 10,
        person_at: 10,
        other,
        ..Latest::default()
    };
    vec![
        (
            "an invitation from a correspondent",
            from(MessageFacts {
                invite: true,
                ..person
            }),
            Section::Other,
        ),
        (
            "a sender I never write to",
            from(MessageFacts {
                mutual: false,
                ..person
            }),
            Section::Other,
        ),
        (
            "a sender I write to a fifth as often as they write to me",
            from(MessageFacts {
                mutual: mutual(1, 5),
                ..person
            }),
            Section::Reply,
        ),
        (
            "a sender I write to less than a fifth as often",
            from(MessageFacts {
                mutual: mutual(1, 6),
                ..person
            }),
            Section::Other,
        ),
        (
            "a one-way sender, in a conversation I wrote in",
            Latest {
                own_at: 5,
                own_to_others: true,
                ..from(MessageFacts {
                    mutual: false,
                    ..person
                })
            },
            Section::Reply,
        ),
        (
            "an invitation in a conversation I wrote in",
            Latest {
                own_at: 5,
                own_to_others: true,
                ..from(MessageFacts {
                    invite: true,
                    ..person
                })
            },
            Section::Other,
        ),
        (
            "a one-way sender, copied, in a conversation I wrote in",
            Latest {
                own_at: 5,
                own_to_others: true,
                ..from(MessageFacts {
                    direct: false,
                    mutual: false,
                    ..person
                })
            },
            Section::Other,
        ),
    ]
}
