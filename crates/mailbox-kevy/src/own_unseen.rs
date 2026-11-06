//! Own mail stored without `\Seen`, from before own mail could count as
//! unread.
//!
//! The group a message row counts under is written onto the row, and own
//! mail without `\Seen` used to be grouped as never unread. Rows like that
//! exist: Sent copies filed before ingest wrote `\Seen` on them, IMAP
//! APPENDs without the flag, and the newest message of a conversation the
//! reader marked unread. Each one is regrouped whenever its row is
//! rewritten (a flag change, a re-ingest, a split, `group-backfill`), and
//! from then on its conversation shows unread mail nobody marked unread.
//!
//! [`settle_own_unseen`](KevyMailboxStore::settle_own_unseen) sets `\Seen`
//! on them, which is what ingest writes on own mail today. Every
//! conversation then shows the unread count it showed before, except one
//! the reader marked unread whose only unseen mail is their own: it keeps
//! its newest own unseen message unseen, which is the state marking it
//! unread produces now, so it stays unread with a count of one.

use std::io;

use mailrs_mailbox::types::FLAG_SEEN;

use crate::KevyMailboxStore;
use crate::keys;

/// One conversation of one reader, as the step finds it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OwnUnseen {
    /// Own messages without `\Seen`.
    pub rows: u64,
    /// Unread messages under the old rule, which never counted own mail.
    pub before: u64,
    /// Unread messages under the current rule, with the rows as they are.
    pub as_is: u64,
    /// Unread messages under the current rule once the step has run.
    pub after: u64,
    /// The membership row's `unread` column: in the Unread list.
    pub marked_unread: bool,
    /// `(blob_ref, flags)` of each row the step sets `\Seen` on, for the
    /// caller to rename the file. Listed on a dry run too.
    pub seen: Vec<(String, u32)>,
}

impl KevyMailboxStore {
    /// Set `\Seen` on `user`'s own unseen mail in `thread_id`, except the
    /// newest one when the conversation is marked unread and nothing else
    /// in it is unread. Writes nothing when `write` is false and answers
    /// the same.
    ///
    /// Converges: on a second call every remaining own unseen row is the
    /// one kept, so `seen` is empty.
    pub fn settle_own_unseen(
        &self,
        user: &str,
        thread_id: &str,
        write: bool,
    ) -> io::Result<OwnUnseen> {
        let mut out = OwnUnseen::default();
        let mut own_unseen = Vec::new();
        for mid in self.user_thread_message_ids(user, thread_id)? {
            let Some(mut facts) = self.user_message_facts(user, &mid)? else {
                continue;
            };
            if facts.flags & FLAG_SEEN != 0 {
                continue;
            }
            // Rows written before the `own` column carry no answer of their
            // own; the shared blob is what the group backfill would read.
            let own = facts.own || self.own_by_blob(user, &mid)?;
            match own {
                true => {
                    facts.own = true;
                    facts.tid = thread_id.to_string();
                    own_unseen.push((mid, facts));
                }
                false => out.before += 1,
            }
        }
        out.rows = own_unseen.len() as u64;
        out.as_is = out.before + out.rows;
        let column = self
            .store()
            .hget(keys::thread_user(user, thread_id).as_bytes(), b"unread")?;
        out.marked_unread = column.as_deref() == Some(b"1".as_slice());

        // `user_thread_message_ids` is oldest first.
        let keep = out.marked_unread && out.before == 0 && own_unseen.pop().is_some();
        out.after = out.before + u64::from(keep);
        for (mid, facts) in own_unseen {
            let flags = facts.flags | FLAG_SEEN;
            if write {
                self.write_flags(user, &mid, &facts, flags)?;
            }
            out.seen.push((facts.blob_ref, flags));
        }
        Ok(out)
    }

    fn own_by_blob(&self, user: &str, message_id: &str) -> io::Result<bool> {
        let blob = self
            .store()
            .get(keys::message_blob(message_id).as_bytes())?;
        Ok(blob.is_some_and(|b| crate::messages::own_from_payload(&b, user)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::count_shadow::tests::*;

    const U: &str = "u@x.com";

    fn unread_now(s: &KevyMailboxStore, tid: &str) -> i64 {
        s.counts_from_index(U, tid).map(|(_, u, _)| u).unwrap_or(0)
    }

    /// An own unseen row in a conversation nobody marked unread is set
    /// seen, so regrouping it can no longer turn the conversation unread.
    #[test]
    fn own_unseen_mail_nobody_marked_unread_is_set_seen() {
        let s = store();
        arrive(&s, "t1", U, false, true);
        put(&s, U, "t1", "m1", 0, U);
        assert_eq!(unread_now(&s, "t1"), 1, "regrouped, it reads unread");

        let dry = s.settle_own_unseen(U, "t1", false).unwrap();
        assert_eq!((dry.rows, dry.before, dry.as_is, dry.after), (1, 0, 1, 0));
        assert!(!dry.marked_unread);
        assert_eq!(dry.seen.len(), 1);
        assert_eq!(unread_now(&s, "t1"), 1, "a dry run writes nothing");

        assert_eq!(s.settle_own_unseen(U, "t1", true).unwrap(), dry);
        assert_eq!(unread_now(&s, "t1"), 0);
        let again = s.settle_own_unseen(U, "t1", true).unwrap();
        assert_eq!(
            (again.rows, again.seen.len()),
            (0, 0),
            "a second run does nothing"
        );
    }

    /// A conversation the reader marked unread, holding only their own
    /// mail, keeps exactly its newest own message unseen.
    #[test]
    fn a_conversation_marked_unread_keeps_its_newest_own_message_unseen() {
        let s = store();
        arrive(&s, "t1", U, false, true);
        put(&s, U, "t1", "m1", 0, U);
        put(&s, U, "t1", "m2", 0, U);
        s.mark_unread(U, "t1").unwrap();

        let done = s.settle_own_unseen(U, "t1", true).unwrap();
        assert_eq!((done.rows, done.before, done.after), (2, 0, 1));
        assert!(done.marked_unread);
        assert_eq!(unread_now(&s, "t1"), 1);
        let m1 = s.user_message_facts(U, "m1").unwrap().unwrap().flags;
        let m2 = s.user_message_facts(U, "m2").unwrap().unwrap().flags;
        assert_eq!((m1 & FLAG_SEEN != 0, m2 & FLAG_SEEN != 0), (true, false));
        assert!(
            s.settle_own_unseen(U, "t1", true).unwrap().seen.is_empty(),
            "the kept row is kept again"
        );
    }

    /// With other unread mail beside it, the conversation is unread anyway
    /// and own mail adds nothing to its count, as before.
    #[test]
    fn own_unseen_mail_beside_unread_mail_is_set_seen() {
        let s = store();
        arrive(&s, "t1", U, true, false);
        put(&s, U, "t1", "m1", 0, "other@z.com");
        put(&s, U, "t1", "m2", 0, U);
        s.mark_unread(U, "t1").unwrap();

        let done = s.settle_own_unseen(U, "t1", true).unwrap();
        assert_eq!(
            (done.rows, done.before, done.as_is, done.after),
            (1, 1, 2, 1)
        );
        assert_eq!(unread_now(&s, "t1"), 1);
    }

    /// Ownership the row does not carry is read from the shared blob, the
    /// way the group backfill decides it.
    #[test]
    fn a_row_without_the_own_column_is_judged_by_its_blob() {
        let s = store();
        arrive(&s, "t1", U, false, true);
        put(&s, U, "t1", "m1", 0, U);
        s.store()
            .hdel(keys::user_message(U, "m1").as_bytes(), &[b"own".as_slice()])
            .unwrap();
        assert!(!s.user_message_facts(U, "m1").unwrap().unwrap().own);

        let done = s.settle_own_unseen(U, "t1", true).unwrap();
        assert_eq!(done.rows, 1);
        assert_eq!(unread_now(&s, "t1"), 0);
    }
}
