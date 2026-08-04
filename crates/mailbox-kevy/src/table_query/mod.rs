//! Reading the declared table — one function per axis the conversation
//! list can ask for.
//!
//! The composite encoding is what makes these cheap: `activity` sits right
//! after the equality columns, so a cursor is a range on the component the
//! ORDERPATH was designed to range over, not a scan with a filter on top.
//!
//! Split by which index answers the question: [`orderpath`] for the
//! composite paths the list pages through, [`flag`] for the boolean axes
//! that key on one column and reach the rest as stored values. The clause
//! plumbing both share stays here, where each can see it.

use std::io;

use super::KevyMailboxStore;
use super::keys;
use super::table_spec::thread_user_spec;

mod flag;
mod orderpath;

/// Whether a read over an ORDERPATH includes archived threads.
///
/// Archived is a list of its own — cross-folder, reached by keying on
/// the `archived` flag index — so every *other* list excludes it, and
/// [`ArchiveScope::Live`] is what a user-facing read wants. `All` is for
/// the maintenance sweeps whose job is to visit every row once.
///
/// `archived` is an equality component of every ORDERPATH prefix, ahead
/// of `activity`. That is what makes `Live` an index range rather than a
/// post-filter — and it is also why `All` cannot page: leaving the
/// component unconstrained orders the whole subtree by
/// `(archived, activity)`, so a cursor on `activity` would be reading a
/// sort that does not exist. The helpers that take a cursor say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArchiveScope {
    /// Everything the user has not archived.
    #[default]
    Live,
    /// Archived and live alike. Sweeps and audits, never a list.
    All,
}

impl ArchiveScope {
    /// The equality predicate this scope contributes, if any.
    fn eq(self) -> Option<(Vec<u8>, Vec<u8>)> {
        match self {
            Self::Live => Some((b"archived".to_vec(), b"0".to_vec())),
            Self::All => None,
        }
    }

    /// Append it to a clause's equality columns, in prefix order — the
    /// caller has already pushed everything that comes before it.
    fn push_to(self, eqs: &mut Vec<(Vec<u8>, Vec<u8>)>) {
        if let Some(pair) = self.eq() {
            eqs.push(pair);
        }
    }

    /// A cursor read needs `archived` pinned, or the range below it is
    /// on a sort the index does not hold.
    fn require_pinned(self, what: &str) -> io::Result<()> {
        match self {
            Self::Live => Ok(()),
            Self::All => Err(io::Error::other(format!(
                "{what}: ArchiveScope::All cannot take a cursor — the ORDERPATH \
                 keys on `archived` ahead of `activity`"
            ))),
        }
    }
}

impl KevyMailboxStore {
    fn bucket_bounds(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
        scope: ArchiveScope,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        self.composite_bounds_for(
            index,
            &Self::scoped_clause(user, second_col, second_val, scope),
        )
    }

    fn query_orderpath(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
        scope: ArchiveScope,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.run_orderpath(
            index,
            user,
            &Self::scoped_clause(user, second_col, second_val, scope),
            limit,
        )
    }

    /// `(user, <second>, archived?)` — the equality prefix both the
    /// page and its count are built from, so the two cannot disagree
    /// about what they are counting.
    fn scoped_clause(
        user: &str,
        second_col: &[u8],
        second_val: &str,
        scope: ArchiveScope,
    ) -> kevy_index::WhereClause {
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (second_col.to_vec(), second_val.as_bytes().to_vec()),
        ];
        scope.push_to(&mut eqs);
        kevy_index::WhereClause { eqs, range: None }
    }

    /// Resolve the composite columns for `index` from the declared
    /// spec, so the bounds are always encoded the way the segment was.
    fn composite_bounds_for(
        &self,
        index: &[u8],
        clause: &kevy_index::WhereClause,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        use kevy_index::{CompositeCol, composite_bounds};
        let spec = thread_user_spec();
        let path = spec
            .orderpaths
            .iter()
            .find(|p| index.ends_with(&p.name))
            .ok_or_else(|| io::Error::other("no such orderpath in the spec"))?;
        let cols: Vec<CompositeCol> = path
            .on
            .iter()
            .map(|(col, desc)| {
                let ty = spec
                    .column_type(col)
                    .ok_or_else(|| io::Error::other("orderpath names an undeclared column"))?;
                Ok(CompositeCol {
                    name: col.clone(),
                    ty,
                    desc: *desc,
                })
            })
            .collect::<io::Result<_>>()?;
        composite_bounds(&cols, clause).map_err(io::Error::other)
    }

    fn run_orderpath(
        &self,
        index: &[u8],
        user: &str,
        clause: &kevy_index::WhereClause,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let (lo, hi) = self.composite_bounds_for(index, clause)?;
        let (rows, _cursor) = self
            .store
            .idx_query(
                index,
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
                None,
                limit,
            )
            .map_err(io::Error::other)?;

        // The row key is `mailrs:threaduser:{user}:{tid}`; the tid can
        // itself contain colons (it is a Message-ID), so split off the
        // known prefix rather than splitting on the separator.
        let prefix_len = keys::thread_user(user, "").len();
        Ok(rows
            .into_iter()
            .filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            })
            .collect())
    }
}
