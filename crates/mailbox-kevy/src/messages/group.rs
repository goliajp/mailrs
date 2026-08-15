//! The group key a declared aggregate index counts by.
//!
//! `count`, `unread_count` and `sent_count` are three numbers maintained by
//! hand on two rows today, and everything that exists to repair their drift
//! — `recount-threads`, `shadow-counts`, `repair_thread_counts` and its
//! four variants, the `unread` axis column, reindex's counts leg — exists
//! because they are *stored*. An engine that derives them from the rows
//! cannot drift, so the counters move into a `KIND agg` index and this is
//! the column it groups by.
//!
//! **Why a composite string and not three columns.** `IndexSpec::group_by`
//! takes exactly one field name, and `KIND agg` has no predicate — the
//! `excluded` counter in `AggStats` counts coerce failures, not rows a
//! filter skipped. So "count only the unseen ones" cannot be *asked*; it
//! has to be answered by which group a row lands in. The composite is
//! forced by the engine, and `rules/kevy-patterns.md` →
//! `kevy/orderpath-not-another-column` is satisfied the only way it can be:
//! this is the single function that produces one, so a row's group can
//! never disagree with the row.
//!
//! **Why three states and not two flags.** From `message_arrival.rs`, the
//! three counters are not three independent sums:
//!
//! ```text
//!   count        += 1                       always
//!   sent_count   += 1  if is_own
//!   unread_count += 1  if unread && !is_own
//! ```
//!
//! A message you sent is **never unread for you**. Encoding seen-ness and
//! ownership as two independent flags would make four groups and leave that
//! rule to be re-applied at every read; one three-valued state makes three
//! groups and keeps the rule here.

use mailrs_mailbox::types::FLAG_SEEN;

/// Which of the three the row counts toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Not seen, and not the user's own send — the only thing `unread_count`
    /// counts.
    Unread,
    /// Seen, and not the user's own send.
    Read,
    /// Sent by this user. Counts toward `sent_count`, and never toward
    /// `unread_count` however the `\Seen` bit reads.
    Own,
}

impl State {
    /// The one place the rule lives.
    pub(crate) fn of(flags: u32, own: bool) -> State {
        match (own, flags & FLAG_SEEN != 0) {
            (true, _) => State::Own,
            (false, true) => State::Read,
            (false, false) => State::Unread,
        }
    }

    /// The byte that goes in the key. Short because it is stored on every
    /// per-user message row and repeated in every group name.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            State::Unread => "u",
            State::Read => "r",
            State::Own => "o",
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
    serde_json::from_slice::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("sender")
                .and_then(|s| s.as_str())
                .map(|s| crate::senders_csv_contains_user(s, user))
        })
        .unwrap_or(false)
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
/// `unread_count = count(Unread)`, `sent_count = count(Own)`,
/// `count = count(Unread) + count(Read) + count(Own)`.
pub(crate) fn group_name(user: &str, tid: &str, state: State) -> String {
    format!("{user}\0{tid}\0{}", state.tag())
}

/// All three, for a test that asserts a writer can only produce these.
#[cfg(test)]
pub(crate) fn group_names(user: &str, tid: &str) -> [String; 3] {
    [
        group_name(user, tid, State::Unread),
        group_name(user, tid, State::Read),
        group_name(user, tid, State::Own),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule this module exists to hold: your own message is not unread,
    /// whatever its `\Seen` bit says.
    ///
    /// Both directions matter. An unread-looking own message is the case
    /// that inflates a badge nobody can clear — the user cannot "read" a
    /// message they sent — and it is exactly what `message_arrival` guards
    /// with `unread && !is_own`.
    #[test]
    fn your_own_message_is_never_unread() {
        assert_eq!(State::of(0, true), State::Own);
        assert_eq!(State::of(FLAG_SEEN, true), State::Own);
        assert_eq!(State::of(0, false), State::Unread);
        assert_eq!(State::of(FLAG_SEEN, false), State::Read);
    }

    /// Flags other than `\Seen` do not move a row between groups. A starred
    /// unread message is still unread.
    #[test]
    fn only_the_seen_bit_decides() {
        let other = !FLAG_SEEN;
        assert_eq!(State::of(other, false), State::Unread);
        assert_eq!(State::of(other | FLAG_SEEN, false), State::Read);
    }

    /// The three names a reader asks for are the three a writer can produce,
    /// or a counter silently reads zero from a group nothing writes.
    #[test]
    fn every_state_has_a_name_a_reader_asks_for() {
        let names = group_names("u@x.com", "t1");
        for (flags, own) in [(0, false), (FLAG_SEEN, false), (0, true)] {
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
        let (u, r, o) = (g(State::Unread), g(State::Read), g(State::Own));
        (u + r + o, u, o)
    }

    /// The engine's three counts are the three the hand-maintained row
    /// carries — that agreement is the whole case for the change.
    ///
    /// Deliberately uneven: two unread, one read, one the user's own, and
    /// the own one left *unseen* so a rule that forgot "your own message is
    /// never unread" would show up as an unread count of three.
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
        put(&s, u, tid, "m4", 0, "u@x.com");

        assert_eq!(counted(&s, u, tid), (4, 2, 1));

        // Reading one moves it between groups, and the total does not move.
        s.mark_user_message_seen(u, "m1").unwrap();
        assert_eq!(counted(&s, u, tid), (4, 1, 1));

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
