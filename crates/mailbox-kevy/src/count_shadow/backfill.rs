//! The backfill that writes the group column onto rows that predate it.
//!
//! A submodule rather than a sibling: it reads the shadow's test helpers
//! and the parent's private items, and in Rust a child sees its parent's
//! privates while two siblings see neither's. Split out under the 500-line
//! rule (`rules/common/file-size.md`), which refused the 2.70.2 deploy at
//! 540 — the ratchet working as intended, since the baseline is empty and
//! is meant to stay that way.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Write `tid`, `own` and `g` onto rows that predate them.
    ///
    /// Stage C3. Every row on production was written before the group
    /// column existed, so the index cannot see any of them and the engine
    /// counts short for every thread. This is what closes that.
    ///
    /// **`own` comes from [`crate::messages::own_from_payload`]**, the same
    /// function `upsert_user_message` calls. A backfill that decided
    /// ownership its own way would converge the shadow onto a number that
    /// is wrong in a new way, and nothing downstream could tell.
    ///
    /// **Conditional, so it converges.** A row already carrying the right
    /// group is skipped and not counted, so a second pass over a repaired
    /// mailbox does no writes and reports none — `rules/periodic-work-must-converge.md`.
    /// The maildir self-heal `zadd`'d unconditionally every 31 seconds and
    /// logged `sent_added=255 created=0` forever, which is how a sweep that
    /// does no useful work hides the fact.
    ///
    /// Returns `(rows_seen, rows_written)`. With `write = false` it walks
    /// the identical rows and answers the identical question — how many
    /// would change — without changing them. **One function, one switch**:
    /// reindex's dry run once asked a different question from its real run
    /// and reported zero, and that zero was read on production as a clean
    /// mailbox.
    pub fn backfill_group_columns(&self, user: &str, thread_id: &str) -> io::Result<(u64, u64)> {
        self.backfill_group_columns_inner(user, thread_id, true)
    }

    /// The dry run: same walk, same comparison, no writes.
    pub fn count_group_columns_to_write(
        &self,
        user: &str,
        thread_id: &str,
    ) -> io::Result<(u64, u64)> {
        self.backfill_group_columns_inner(user, thread_id, false)
    }

    fn backfill_group_columns_inner(
        &self,
        user: &str,
        thread_id: &str,
        write: bool,
    ) -> io::Result<(u64, u64)> {
        let mut seen = 0u64;
        let mut written = 0u64;
        for mid in self.user_thread_message_ids(user, thread_id)? {
            seen += 1;
            let key = keys::user_message(user, &mid);
            let flat = self.store().hgetall(key.as_bytes())?;
            if flat.is_empty() {
                continue;
            }
            let mut flags = 0u32;
            let mut have_group = None;
            for (f, v) in &flat {
                match std::str::from_utf8(f).unwrap_or("") {
                    "flags" => flags = String::from_utf8_lossy(v).parse().unwrap_or(0),
                    "g" => have_group = Some(String::from_utf8_lossy(v).to_string()),
                    _ => {}
                }
            }
            // Ownership is a fact about the message, and the shared blob is
            // where the sender lives.
            let own = match self.store().get(keys::message_blob(&mid).as_bytes())? {
                Some(blob) => crate::messages::own_from_payload(&blob, user),
                None => false,
            };
            let group = crate::messages::group_key(user, thread_id, flags, own);
            if have_group.as_deref() == Some(group.as_str()) {
                continue;
            }
            if write {
                self.store().hset(
                    key.as_bytes(),
                    &[
                        (b"tid".as_slice(), thread_id.as_bytes()),
                        (b"own".as_slice(), if own { b"1".as_slice() } else { b"0" }),
                        (b"g".as_slice(), group.as_bytes()),
                    ],
                )?;
            }
            written += 1;
        }
        Ok((seen, written))
    }
}

#[cfg(test)]
mod backfill_tests {
    use super::*;
    use crate::count_shadow::tests::*;

    /// The backfill makes the engine agree, and a second pass writes
    /// nothing — a sweep that redoes its work every cycle is a busy-wait.
    #[test]
    fn the_backfill_converges_and_then_does_nothing() {
        let s = store();
        let u = "u@x.com";
        // One arrival per message, or the stored counter says one and the
        // engine counts two — a disagreement in the fixture, not the code.
        arrive(&s, "t1", u, true, false);
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        put(&s, u, "t1", "m2", 0, "other@z.com");
        // Strip both rows the way every production row lacks the column.
        for mid in ["m1", "m2"] {
            s.store()
                .hdel(
                    keys::user_message(u, mid).as_bytes(),
                    &[
                        keys::USER_MESSAGE_GROUP_FIELD,
                        b"tid".as_slice(),
                        b"own".as_slice(),
                    ],
                )
                .unwrap();
        }
        let before = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!(before.ungrouped_rows, 2);

        assert_eq!(s.backfill_group_columns(u, "t1").unwrap(), (2, 2));

        let after = s.shadow_declared_counts(u, 0, 100).unwrap();
        assert_eq!(after.ungrouped_rows, 0, "the debt is gone");
        assert_eq!(after.agreed, 1, "and the engine now agrees");

        // Convergence: nothing left to do, and it says so.
        assert_eq!(
            s.backfill_group_columns(u, "t1").unwrap(),
            (2, 0),
            "a second pass must write nothing"
        );
    }

    /// The backfill's idea of ownership is the write path's idea of it.
    ///
    /// If these ever diverge the shadow converges onto a wrong number and
    /// nothing downstream can tell, so the agreement is asserted directly
    /// rather than inferred from a count.
    #[test]
    fn the_backfill_agrees_with_the_write_path_about_ownership() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        // One inbound, one the user sent.
        put(&s, u, "t1", "m1", 0, "other@z.com");
        put(&s, u, "t1", "m2", 0, "u@x.com");

        let written_by_upsert: Vec<bool> = ["m1", "m2"]
            .iter()
            .map(|m| s.user_message_facts(u, m).unwrap().unwrap().own)
            .collect();
        assert_eq!(written_by_upsert, vec![false, true]);

        for mid in ["m1", "m2"] {
            s.store()
                .hdel(
                    keys::user_message(u, mid).as_bytes(),
                    &[keys::USER_MESSAGE_GROUP_FIELD, b"own".as_slice()],
                )
                .unwrap();
        }
        s.backfill_group_columns(u, "t1").unwrap();

        let written_by_backfill: Vec<bool> = ["m1", "m2"]
            .iter()
            .map(|m| s.user_message_facts(u, m).unwrap().unwrap().own)
            .collect();
        assert_eq!(
            written_by_backfill, written_by_upsert,
            "the two writers disagree about who sent what"
        );
    }
}

#[cfg(test)]
mod dry_run_tests {
    use super::*;
    use crate::count_shadow::tests::*;

    /// **The dry run answers the real run's question.** reindex's did not:
    /// it checked one leg of four, reported zero, and that zero was read on
    /// production as a clean mailbox. Both modes go through one function
    /// with a switch, and this asserts the two numbers are the same one.
    #[test]
    fn the_dry_run_counts_what_the_real_run_writes() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        put(&s, u, "t1", "m2", 0, "other@z.com");
        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();

        let dry = s.count_group_columns_to_write(u, "t1").unwrap();
        assert_eq!(dry, (2, 1), "one row needs the column");
        // And it really did not write.
        assert_eq!(s.count_group_columns_to_write(u, "t1").unwrap(), dry);

        assert_eq!(
            s.backfill_group_columns(u, "t1").unwrap(),
            dry,
            "the real run must write exactly what the dry run counted"
        );
        assert_eq!(s.count_group_columns_to_write(u, "t1").unwrap(), (2, 0));
    }
}
