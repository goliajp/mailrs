//! Who a conversation is with, and crediting what a reader does with it to
//! that person rather than to the reader.
//!
//! A conversation's participants are a union of every sender, the reader's
//! own address among them once they reply, first on a thread they started.
//! Engagement belongs to the correspondent, so taking the first address
//! would credit the reader on exactly the threads that matter most.

use super::contacts::{Engagement, record_engagement};

/// A display-name form (`Alice <a@x.com>`) down to the address, lowercased.
pub fn bare_address(token: &str) -> String {
    let t = token.trim();
    let bare = match (t.rfind('<'), t.rfind('>')) {
        (Some(open), Some(close)) if close > open + 1 => &t[open + 1..close],
        _ => t,
    };
    bare.trim().to_lowercase()
}

/// The first address in a comma-separated list, lowercased.
pub fn first_address(csv: &str) -> String {
    bare_address(csv.split(',').next().unwrap_or(""))
}

/// Every address in a comma-separated list, lowercased; anything without
/// an `@` dropped.
pub fn all_addresses(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(bare_address)
        .filter(|a| !a.is_empty() && a.contains('@'))
        .collect()
}

/// The first participant who is not `user`, or `None` when the reader is
/// the only one.
pub fn correspondent(participants: &str, user: &str) -> Option<String> {
    let me = user.trim().to_lowercase();
    all_addresses(participants).into_iter().find(|a| *a != me)
}

/// A read this soon after the latest arrival counts as fast: same-sitting
/// attention, "the reader was waiting for this", not reflexes.
pub const FAST_READ_SECS: i64 = 600;

/// What a reader did to a conversation, as engagement sees it.
#[derive(Debug, Clone, Copy)]
pub enum ReaderAct {
    /// Marked it read, from any client; `now` in unix seconds.
    Read {
        now: i64,
    },
    Starred,
    Archived,
    MarkedJunk,
}

/// The reader's conversation as it stood just before the act.
#[derive(Debug, Clone, Copy, Default)]
pub struct Before {
    pub unread: bool,
    /// Unix seconds of the latest arrival; 0 when unknown.
    pub latest: i64,
}

/// What `act` on a conversation in state `before` says about its
/// correspondent, or `None` when it says nothing. Only a genuine unread to
/// read change is a read: a client re-marking an open conversation measures
/// UI chatter, not attention. Archiving something still unread is the
/// reader dismissing it unseen. Both cores decide with this, so the ranker
/// learns the same from either.
pub fn engagement_of(act: ReaderAct, before: Before) -> Option<Engagement> {
    match act {
        ReaderAct::Read { now } if before.unread => Some(Engagement::Read {
            fast: before.latest > 0
                && now >= before.latest
                && now - before.latest <= FAST_READ_SECS,
        }),
        ReaderAct::Read { .. } => None,
        ReaderAct::Starred => Some(Engagement::Starred),
        ReaderAct::Archived if before.unread => Some(Engagement::ArchivedUnread),
        ReaderAct::Archived => None,
        ReaderAct::MarkedJunk => Some(Engagement::MarkedJunk),
    }
}

/// Record `event` against the conversation's correspondent. A conversation
/// with nobody else in it records nothing.
pub fn record_thread_engagement(
    conn: &mut kevy_client::Connection,
    user: &str,
    participants: &str,
    event: Engagement,
) -> std::io::Result<()> {
    match correspondent(participants, user) {
        Some(sender) => record_engagement(conn, user, &sender, event),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_real_change_is_engagement() {
        let unread = Before {
            unread: true,
            latest: 1_000,
        };
        let read = Before {
            unread: false,
            latest: 1_000,
        };
        assert_eq!(
            engagement_of(
                ReaderAct::Read {
                    now: 1_000 + FAST_READ_SECS
                },
                unread
            ),
            Some(Engagement::Read { fast: true })
        );
        assert_eq!(
            engagement_of(
                ReaderAct::Read {
                    now: 1_001 + FAST_READ_SECS
                },
                unread
            ),
            Some(Engagement::Read { fast: false })
        );
        assert_eq!(
            engagement_of(ReaderAct::Read { now: 999 }, unread),
            Some(Engagement::Read { fast: false }),
            "a clock behind the arrival is not fast"
        );
        assert_eq!(
            engagement_of(ReaderAct::Read { now: 1_000 }, read),
            None,
            "re-marking a read conversation"
        );
        assert_eq!(
            engagement_of(ReaderAct::Archived, unread),
            Some(Engagement::ArchivedUnread)
        );
        assert_eq!(engagement_of(ReaderAct::Archived, read), None);
        assert_eq!(
            engagement_of(ReaderAct::Starred, read),
            Some(Engagement::Starred)
        );
        assert_eq!(
            engagement_of(ReaderAct::MarkedJunk, read),
            Some(Engagement::MarkedJunk)
        );
    }

    #[test]
    fn engagement_goes_to_the_correspondent_not_the_reader() {
        let me = "lihao@golia.jp";
        assert_eq!(
            correspondent("lihao@golia.jp, Alice <alice@x.com>", me).as_deref(),
            Some("alice@x.com")
        );
        assert_eq!(
            correspondent("Alice <alice@x.com>, lihao@golia.jp", me).as_deref(),
            Some("alice@x.com")
        );
        assert_eq!(
            correspondent("LIHAO@GOLIA.JP, bob@y.com", me).as_deref(),
            Some("bob@y.com")
        );
        assert_eq!(correspondent("lihao@golia.jp", me), None);
        assert_eq!(correspondent("", me), None);
    }

    #[test]
    fn addresses_are_split_and_normalised() {
        assert_eq!(
            all_addresses("Alice <A@X.com>, bob@y.com"),
            vec!["a@x.com".to_string(), "bob@y.com".to_string()]
        );
        assert_eq!(
            all_addresses("undisclosed-recipients:;"),
            Vec::<String>::new()
        );
        assert_eq!(all_addresses(""), Vec::<String>::new());
        assert_eq!(first_address("Alice <A@X.com>"), "a@x.com");
        assert_eq!(first_address("Alice <a@x.com>, bob@y.com"), "a@x.com");
        assert_eq!(first_address(""), "");
    }
}
