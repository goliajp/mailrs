//! Marker columns: the sparse twins the per-user flag indexes key on.
//!
//! kevy indexes every row whose column is present, a stored 0 included,
//! and every read of these axes asks for non-zero values only. So each
//! indexed flag or timestamp has a marker, `ix_<column>`, that carries the
//! column's value while it is non-zero and is absent otherwise: its index
//! holds only the rows a query can return, instead of every row in the
//! table.
//!
//! The source column stays as it is. It is a stored value beside every
//! index and an equality component of the orderpaths, and an orderpath
//! leaves out a row that lacks any of its columns.
//!
//! A marker that disagrees with its column takes the row out of a list (or
//! puts it into one) without any error, so every write of a marked column
//! goes through [`hset_membership`], which writes the markers in the same
//! transaction.

use kevy_embedded::{AtomicCtx, KevyResult};

/// One indexed column and the marker its index keys on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Marked {
    pub(crate) column: &'static str,
    pub(crate) marker: &'static str,
}

const fn marked(column: &'static str, marker: &'static str) -> Marked {
    Marked { column, marker }
}

/// Every column whose index keys on a marker. `sec_in_from` and
/// `account_id` are not here: most of their rows carry a value the reads
/// ask for, so there is nothing to leave out.
pub(crate) const MARKED: [Marked; 10] = [
    marked("starred", "ix_starred"),
    marked("archived", "ix_archived"),
    marked("trashed", "ix_trashed"),
    marked("pinned", "ix_pinned"),
    marked("unread", "ix_unread"),
    marked("has_action", "ix_has_action"),
    marked("is_sender", "ix_is_sender"),
    marked("snoozed_until", "ix_snoozed_until"),
    marked("trashed_at", "ix_trashed_at"),
    marked("awaiting_since", "ix_awaiting_since"),
];

/// The marker for `column`, or `None` when its index keys on the column
/// itself.
pub(crate) fn marker_of(column: &str) -> Option<&'static str> {
    MARKED.iter().find(|m| m.column == column).map(|m| m.marker)
}

/// The index a read of `column` keys on: the marker's where there is one.
pub(crate) fn index_for(column: &str) -> String {
    format!("threaduser.{}", marker_of(column).unwrap_or(column))
}

/// Whether a stored value puts the row on the marker's index: a non-zero
/// integer, the only values the reads ask for.
pub(crate) fn is_set(value: &[u8]) -> bool {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .is_some_and(|n| n != 0)
}

/// Marker writes: the markers to write with their values, and the markers
/// to remove.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MarkerWrites<V> {
    pub(crate) set: Vec<(&'static [u8], V)>,
    pub(crate) clear: Vec<&'static [u8]>,
}

impl<V> MarkerWrites<V> {
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty() && self.clear.is_empty()
    }
}

/// What the markers of a row must be once `pairs` are written.
fn marker_writes<'a>(pairs: &[(&[u8], &'a [u8])]) -> MarkerWrites<&'a [u8]> {
    let mut w = MarkerWrites::default();
    for (field, value) in pairs {
        let Some(m) = MARKED.iter().find(|m| m.column.as_bytes() == *field) else {
            continue;
        };
        match is_set(value) {
            true => w.set.push((m.marker.as_bytes(), *value)),
            false => w.clear.push(m.marker.as_bytes()),
        }
    }
    w
}

/// Write `pairs` onto a membership row inside the caller's transaction,
/// with the markers they imply: a non-zero marked column writes its
/// marker with the same value, a zero one removes the marker.
///
/// Removing rather than writing 0 is the point: a marker stored as 0 is
/// an index entry again.
pub fn hset_membership(
    ctx: &mut AtomicCtx<'_>,
    key: &[u8],
    pairs: &[(&[u8], &[u8])],
) -> KevyResult<()> {
    let w = marker_writes(pairs);
    let mut all: Vec<(&[u8], &[u8])> = pairs.to_vec();
    all.extend(w.set);
    ctx.hset(key, &all)?;
    if !w.clear.is_empty() {
        ctx.hdel(key, &w.clear)?;
    }
    Ok(())
}

/// The marker writes that bring an existing row into step with its own
/// columns, empty when it already is.
///
/// For rows written before the markers existed. A marker present with a
/// different value counts as wrong, the same as a missing one.
pub(crate) fn marker_repairs(
    fields: &std::collections::HashMap<Vec<u8>, Vec<u8>>,
) -> MarkerWrites<Vec<u8>> {
    let mut w = MarkerWrites::default();
    for m in &MARKED {
        let source = fields.get(m.column.as_bytes()).filter(|v| is_set(v));
        let have = fields.get(m.marker.as_bytes());
        match (source, have) {
            (Some(want), Some(have)) if want == have => {}
            (Some(want), _) => w.set.push((m.marker.as_bytes(), want.clone())),
            (None, Some(_)) => w.clear.push(m.marker.as_bytes()),
            (None, None) => {}
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_non_zero_integers_are_set() {
        assert!(is_set(b"1"));
        assert!(is_set(b"1782846047"));
        assert!(!is_set(b"0"));
        assert!(!is_set(b""));
        assert!(!is_set(b"x"));
    }

    #[test]
    fn a_zero_clears_and_a_value_sets_with_the_same_bytes() {
        let pairs: [(&[u8], &[u8]); 4] = [
            (b"starred", b"1"),
            (b"trashed_at", b"0"),
            (b"quarantined", b"1"),
            (b"snoozed_until", b"123"),
        ];
        let w = marker_writes(&pairs);
        assert_eq!(
            w.set,
            vec![
                (b"ix_starred".as_slice(), b"1".as_slice()),
                (b"ix_snoozed_until".as_slice(), b"123".as_slice()),
            ]
        );
        assert_eq!(w.clear, vec![b"ix_trashed_at".as_slice()]);
    }

    #[test]
    fn every_marked_column_names_its_own_marker() {
        for m in &MARKED {
            assert_eq!(m.marker, format!("ix_{}", m.column));
        }
        assert_eq!(index_for("starred"), "threaduser.ix_starred");
        assert_eq!(index_for("account_id"), "threaduser.account_id");
    }
}
