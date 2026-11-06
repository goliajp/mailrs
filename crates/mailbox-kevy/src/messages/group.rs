//! The group key a declared aggregate index counts by.
//!
//! `count`, `unread_count` and `sent_count` were three numbers maintained
//! by hand on two rows, and everything that existed to repair their drift
//! — `recount-threads`, `shadow-counts`, `repair_thread_counts` and its
//! four variants, reindex's counts leg — existed because they were
//! *stored*. An engine that derives them from the rows cannot drift, so
//! the counters moved into a `KIND agg` index and this is the column it
//! groups by. All of that repair machinery is deleted (C5c, 2026-08-16);
//! the `unread` axis column is the last piece still hand-maintained.
//!
//! **Why a composite string and not three columns.** `IndexSpec::group_by`
//! takes exactly one field name, and `KIND agg` has no predicate — the
//! `excluded` counter in `AggStats` counts coerce failures, not rows a
//! filter skipped. So "count only the unseen ones" cannot be *asked*; it
//! has to be answered by which group a row lands in. The composite is
//! forced by the engine, and this is the single function that produces one, so a row's group can
//! never disagree with the row.
//!
//! **Why one state and not two flags.** The three counters are not three
//! independent sums:
//!
//! ```text
//!   count        += 1  always
//!   sent_count   += 1  if is_own
//!   unread_count += 1  if not seen
//! ```
//!
//! and one state per row, with a group for each combination a reader asks
//! about, keeps the rule in this file instead of at every read.
//!
//! **Your own message counts as unread only when you marked it so.** A copy
//! of what you sent arrives seen (ingest writes `\Seen` on it), so it lights
//! no badge. Marking it unread afterwards is the reader's own statement, and
//! as in Gmail any conversation can be marked unread, including one holding
//! only your own mail: the row then counts one unread message, as the SQL
//! core counts every unseen one. It used to count an own message as never
//! unread, so a conversation of only your own mail marked unread sat in the
//! Unread list with nothing unread in it.

use mailrs_mailbox::types::FLAG_SEEN;

/// Which group the row counts toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Not seen, and not the user's own send.
    Unread,
    /// Seen, and not the user's own send.
    Read,
    /// Sent by this user, and seen. Counts toward `sent_count` only.
    Own,
    /// Sent by this user, and marked unread since. Counts toward both
    /// `sent_count` and `unread_count`.
    OwnUnread,
}

impl State {
    /// Every state, for a reader adding up a thread's groups.
    pub(crate) const ALL: [State; 4] = [State::Unread, State::Read, State::Own, State::OwnUnread];

    /// The one place the rule lives.
    pub(crate) fn of(flags: u32, own: bool) -> State {
        match (own, flags & FLAG_SEEN != 0) {
            (true, true) => State::Own,
            (true, false) => State::OwnUnread,
            (false, true) => State::Read,
            (false, false) => State::Unread,
        }
    }

    /// Whether `unread_count` counts the row.
    pub(crate) fn unread(self) -> bool {
        matches!(self, State::Unread | State::OwnUnread)
    }

    /// Whether `sent_count` counts the row.
    pub(crate) fn sent(self) -> bool {
        matches!(self, State::Own | State::OwnUnread)
    }

    /// The bytes that go in the key. Short because they are stored on every
    /// per-user message row and repeated in every group name.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            State::Unread => "u",
            State::Read => "r",
            State::Own => "o",
            State::OwnUnread => "ou",
        }
    }
}

/// Whether `user` sent the message this payload describes.
///
/// **One definition, called by both writers.** `upsert_user_message` needs
/// it for every new row and the backfill needs it for every old one, and a
/// backfill that decides ownership differently from the write path would
/// converge the shadow onto a number that is wrong in a new way. The rule
/// is the one `message_arrival` already uses to decide `sent_count`.
///
/// `false` when the payload does not parse or carries no sender: a message
/// nobody can attribute is not this user's own send.
pub(crate) fn own_from_payload(payload: &[u8], user: &str) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return false;
    };
    let text = |field: &str| v.get(field).and_then(|s| s.as_str()).unwrap_or("");
    let (sender, blob_ref) = (text("sender"), text("blob_ref"));
    if sender.is_empty() && blob_ref.is_empty() {
        return false;
    }
    mailrs_mailbox::sent_copy::sent_by(sender, blob_ref, user)
}

/// `user \0 tid \0 state` — what the aggregate index groups by.
///
/// NUL-separated because an address and a thread id are both arbitrary
/// strings and a printable separator could appear inside either. A group
/// name is compared bytewise and never parsed back apart.
pub(crate) fn group_key(user: &str, tid: &str, flags: u32, own: bool) -> String {
    format!("{user}\0{tid}\0{}", State::of(flags, own).tag())
}

/// One thread's group name for a given state — what a reader asks for.
///
/// `unread_count` adds the states [`State::unread`] holds for,
/// `sent_count` those [`State::sent`] holds for, `count` all four.
pub(crate) fn group_name(user: &str, tid: &str, state: State) -> String {
    format!("{user}\0{tid}\0{}", state.tag())
}

/// All four, for a test that asserts a writer can only produce these.
#[cfg(test)]
pub(crate) fn group_names(user: &str, tid: &str) -> [String; 4] {
    State::ALL.map(|st| group_name(user, tid, st))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `\Seen` bit decides whether a row is unread, yours or not, and
    /// ownership whether it is sent. An own message is unread only once the
    /// reader cleared the bit: ingest files it seen.
    #[test]
    fn your_own_message_is_unread_once_marked_so() {
        assert_eq!(State::of(FLAG_SEEN, true), State::Own);
        assert_eq!(State::of(0, true), State::OwnUnread);
        assert_eq!(State::of(0, false), State::Unread);
        assert_eq!(State::of(FLAG_SEEN, false), State::Read);
        let unread: Vec<State> = State::ALL.into_iter().filter(|s| s.unread()).collect();
        let sent: Vec<State> = State::ALL.into_iter().filter(|s| s.sent()).collect();
        assert_eq!(unread, [State::Unread, State::OwnUnread]);
        assert_eq!(sent, [State::Own, State::OwnUnread]);
    }

    /// Flags other than `\Seen` do not move a row between groups. A starred
    /// unread message is still unread.
    #[test]
    fn only_the_seen_bit_decides() {
        let other = !FLAG_SEEN;
        assert_eq!(State::of(other, false), State::Unread);
        assert_eq!(State::of(other | FLAG_SEEN, false), State::Read);
    }

    /// The four names a reader asks for are the four a writer can produce,
    /// or a counter silently reads zero from a group nothing writes.
    #[test]
    fn every_state_has_a_name_a_reader_asks_for() {
        let names = group_names("u@x.com", "t1");
        for (flags, own) in [(0, false), (FLAG_SEEN, false), (FLAG_SEEN, true), (0, true)] {
            let key = group_key("u@x.com", "t1", flags, own);
            assert!(names.contains(&key), "{key:?} is not one of {names:?}");
        }
    }

    /// A separator that could occur inside an address or a thread id would
    /// let two different rows share a group.
    #[test]
    fn the_separator_cannot_occur_in_either_part() {
        let a = group_key("u@x.com", "t1:t2", 0, false);
        let b = group_key("u@x.com:t1", "t2", 0, false);
        assert_ne!(a, b);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::KevyMailboxStore;
    use kevy_embedded::{Config, Store};
    use mailrs_mailbox::types::FLAG_SEEN;
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.ensure_thread_table();
        s.ensure_admin_indexes();
        s
    }

    fn put(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, flags: u32, from: &str) {
        s.upsert_user_message(
            user,
            tid,
            mid,
            100,
            serde_json::json!({ "message_id": mid, "sender": from })
                .to_string()
                .as_bytes(),
            &crate::UserMessageFacts {
                blob_ref: "f.host",
                uid: 1,
                flags,
                modseq: 1,
            },
        )
        .unwrap();
    }

    /// What the engine counts, read the way a reader would.
    fn counted(s: &KevyMailboxStore, user: &str, tid: &str) -> (u64, u64, u64) {
        let g = |st: State| {
            s.store()
                .idx_group(
                    crate::keys::IDX_USERMSG_COUNTS,
                    format!("{user}\0{tid}\0{}", st.tag()).as_bytes(),
                )
                .map(|s| s.count)
                .unwrap_or(0)
        };
        let (mut total, mut unread, mut sent) = (0, 0, 0);
        for st in State::ALL {
            let n = g(st);
            total += n;
            if st.unread() {
                unread += n;
            }
            if st.sent() {
                sent += n;
            }
        }
        (total, unread, sent)
    }

    /// The engine's three counts are the three the hand-maintained row
    /// carries — that agreement is the whole case for the change.
    ///
    /// Deliberately uneven: two unread, one read, one own send filed seen
    /// and one the reader marked unread, which counts as both sent and
    /// unread.
    #[test]
    fn the_engine_counts_what_the_row_says_it_should() {
        let s = store();
        let (u, tid) = ("u@x.com", "t1");
        // The thread hash has to exist, or `delete_thread` returns false
        // before deleting anything and the last assertion below would be
        // measuring a delete that never ran.
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user: u,
            subject: "Subj",
            senders_csv: "other@z.com",
            latest_date: 100,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
        put(&s, u, tid, "m1", 0, "other@z.com");
        put(&s, u, tid, "m2", 0, "other@z.com");
        put(&s, u, tid, "m3", FLAG_SEEN, "other@z.com");
        put(&s, u, tid, "m4", FLAG_SEEN, "u@x.com");
        put(&s, u, tid, "m5", 0, "u@x.com");

        assert_eq!(counted(&s, u, tid), (5, 3, 2));

        // Reading one moves it between groups, and the total does not move.
        s.mark_user_message_seen(u, "m1").unwrap();
        assert_eq!(counted(&s, u, tid), (5, 2, 2));
        // reading your own stays sent
        s.mark_user_message_seen(u, "m5").unwrap();
        assert_eq!(counted(&s, u, tid), (5, 1, 2));

        // Deleting the thread takes its rows, so its groups empty out.
        assert!(
            s.delete_thread(u, tid).unwrap().0,
            "the thread has to have existed for its removal to mean anything"
        );
        assert_eq!(counted(&s, u, tid), (0, 0, 0));
    }

    /// A second user's copies are counted separately, which is the defect
    /// the stored counter cannot express: one shared row cannot hold two
    /// owners' numbers, and `shared_threads: 160` on production is the
    /// measure of it.
    #[test]
    fn two_owners_of_one_thread_get_their_own_counts() {
        let s = store();
        let tid = "shared";
        put(&s, "a@x.com", tid, "m1", 0, "other@z.com");
        put(&s, "a@x.com", tid, "m2", 0, "other@z.com");
        put(&s, "b@x.com", tid, "m1", FLAG_SEEN, "other@z.com");

        assert_eq!(counted(&s, "a@x.com", tid), (2, 2, 0));
        assert_eq!(counted(&s, "b@x.com", tid), (1, 0, 0));
    }
}
