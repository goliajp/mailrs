//! The declared `unread` column of a membership row against the reader's
//! own message rows.
//!
//! The column is what the Unread list and the badge read; the per-thread
//! unread count beside it comes from the aggregate index over the reader's
//! message rows. They are meant to say the same thing. A split used to take
//! a message's read state from its shared blob, which one holder's
//! `mark_seen` had set for every holder, so a split conversation could show
//! an unread count and still be missing from its reader's Unread list.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

/// How a row's column compares with the reader's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnreadColumn {
    Agrees,
    /// The column says read and an unread message of theirs is there.
    TurnsUnread,
    /// The column says unread and every message of theirs is read.
    TurnsRead,
    /// The index holds none of the reader's messages for it.
    IndexSilent,
    /// No membership row.
    NoRow,
}

impl KevyMailboxStore {
    /// Compare `user`'s column for `thread_id` with their messages and,
    /// unless `dry_run`, write the messages' answer. Converges: a second
    /// call after a write answers `Agrees`.
    pub fn recount_unread_column(
        &self,
        user: &str,
        thread_id: &str,
        dry_run: bool,
    ) -> io::Result<UnreadColumn> {
        let key = keys::thread_user(user, thread_id);
        if !self.store().hexists(key.as_bytes(), b"tid")? {
            return Ok(UnreadColumn::NoRow);
        }
        let Some((_, unread, _)) = self.counts_from_index(user, thread_id) else {
            return Ok(UnreadColumn::IndexSilent);
        };
        let have = self.store().hget(key.as_bytes(), b"unread")?;
        let want = unread > 0;
        let verdict = match (have.as_deref() == Some(b"1".as_slice()), want) {
            (true, true) | (false, false) => UnreadColumn::Agrees,
            (false, true) => UnreadColumn::TurnsUnread,
            (true, false) => UnreadColumn::TurnsRead,
        };
        if !dry_run && verdict != UnreadColumn::Agrees {
            self.set_unread_axis(user, thread_id, unread)?;
        }
        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::UnreadColumn;
    use crate::read_state_tests::{BOB, split_after_another_read};

    #[test]
    fn a_split_row_missing_from_unread_is_found_and_put_back_once() {
        let (s, new) = split_after_another_read();
        // what the split before per-reader read state left behind
        s.set_unread_axis(BOB, &new, 0).unwrap();
        assert_eq!(
            s.recount_unread_column(BOB, &new, true).unwrap(),
            UnreadColumn::TurnsUnread
        );
        assert_eq!(
            s.recount_unread_column(BOB, &new, true).unwrap(),
            UnreadColumn::TurnsUnread,
            "a dry run writes nothing"
        );
        assert_eq!(
            s.recount_unread_column(BOB, &new, false).unwrap(),
            UnreadColumn::TurnsUnread
        );
        assert_eq!(
            s.recount_unread_column(BOB, &new, true).unwrap(),
            UnreadColumn::Agrees
        );
        assert_eq!(
            s.count_flag_in_buckets(BOB, "unread", &["inbox"]).unwrap(),
            2
        );
    }

    #[test]
    fn a_read_conversation_marked_unread_turns_read() {
        let (s, new) = split_after_another_read();
        s.mark_seen(BOB, &new).unwrap();
        s.mark_thread_messages_seen(BOB, &new).unwrap();
        s.set_unread_axis(BOB, &new, 1).unwrap();
        assert_eq!(
            s.recount_unread_column(BOB, &new, false).unwrap(),
            UnreadColumn::TurnsRead
        );
        assert_eq!(
            s.recount_unread_column(BOB, "nope", true).unwrap(),
            UnreadColumn::NoRow
        );
    }
}
