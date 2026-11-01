//! The sectioned inbox's axes: one section of the live list off its
//! ORDERPATH, and the follow-up list off the `awaiting_since` index.

use std::io;

use kevy_embedded::IndexValue;
use mailrs_mailbox::section::Section;

use super::ArchiveScope;
use crate::KevyMailboxStore;

impl KevyMailboxStore {
    /// Thread ids in one section, newest first; with `before`, only those
    /// whose activity is older than it.
    pub fn list_thread_ids_by_section_via_table(
        &self,
        user: &str,
        section: Section,
        scope: ArchiveScope,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(
            b"threaduser.by_user_section",
            user,
            b"section",
            section.name(),
            scope,
            limit,
        )
    }

    /// The cursor page of a section.
    pub fn list_thread_ids_by_section_before_via_table(
        &self,
        user: &str,
        section: Section,
        scope: ArchiveScope,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        scope.require_pinned("list_thread_ids_by_section_before_via_table")?;
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (b"section".to_vec(), section.name().as_bytes().to_vec()),
        ];
        scope.push_to(&mut eqs);
        let clause = kevy_index::WhereClause {
            eqs,
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max_activity.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_section", user, &clause, limit)
    }

    /// How many threads are in one section, counted in the index.
    pub fn count_thread_ids_by_section_via_table(
        &self,
        user: &str,
        section: Section,
        scope: ArchiveScope,
    ) -> io::Result<usize> {
        let (lo, hi) = self.bucket_bounds(
            b"threaduser.by_user_section",
            user,
            b"section",
            section.name(),
            scope,
        )?;
        self.store
            .idx_count(
                b"threaduser.by_user_section",
                &IndexValue::Str(lo),
                &IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::from)
    }

    /// `user`'s conversations waiting on a reply since no later than
    /// `due_by` (every waiting one when `None`), with the time each started
    /// waiting, longest-waiting last.
    ///
    /// The index is keyed on the time and holds every reader's rows, so the
    /// range is read whole and narrowed to this reader's key prefix; the
    /// rows themselves are read by the caller for one page only.
    pub fn list_awaiting_via_index(
        &self,
        user: &str,
        due_by: Option<i64>,
    ) -> io::Result<Vec<(String, i64)>> {
        let prefix = crate::keys::thread_user(user, "");
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (hits, next) = self.store().idx_query(
                b"threaduser.awaiting_since",
                &IndexValue::I64(1),
                &IndexValue::I64(due_by.unwrap_or(i64::MAX)),
                cursor.as_ref(),
                10_000,
            )?;
            for (key, value) in hits {
                let Ok(key) = String::from_utf8(key) else {
                    continue;
                };
                let Some(tid) = key.strip_prefix(&prefix) else {
                    continue;
                };
                let since = match value {
                    IndexValue::I64(n) => n,
                    _ => 0,
                };
                out.push((tid.to_string(), since));
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(out)
    }
}
