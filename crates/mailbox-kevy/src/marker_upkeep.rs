//! Bringing existing membership rows' markers into step, and checking that
//! they are.
//!
//! The writers keep the markers (`thread_row::markers`) from the moment
//! they existed; rows written before that carry none. The rewrite fills
//! them in, and the check says whether every marker agrees with its column
//! — the condition for dropping the full-row indexes the markers replace.

use std::collections::HashMap;
use std::io;

use crate::KevyMailboxStore;
use crate::keys;
use crate::thread_row::{MARKED, is_set, marker_repairs};

/// What one page of the rewrite did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct MarkerRewrite {
    /// Membership rows in the table when the page was taken.
    pub total_rows: usize,
    pub walked: usize,
    /// Rows whose markers were written or removed.
    pub changed: usize,
    pub already_correct: usize,
}

/// One marked column across the whole table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MarkerColumnCheck {
    pub column: &'static str,
    /// Rows whose column is non-zero.
    pub rows_set: usize,
    /// Rows carrying the marker.
    pub markers_present: usize,
    /// Entries the marker's index holds, counted by the engine: a second
    /// reading of `markers_present` that does not share its walk.
    pub index_entries: u64,
    /// Rows where the marker is missing, left over, or holds another value.
    pub mismatches: usize,
    /// Up to five of those rows' keys.
    pub samples: Vec<String>,
}

const SAMPLES: usize = 5;

impl KevyMailboxStore {
    /// Every membership row's key, in a stable order so offsets page.
    fn thread_user_keys(&self) -> Vec<Vec<u8>> {
        let pattern = format!("{}*", String::from_utf8_lossy(keys::THREAD_USER_PREFIX));
        let mut all = self.store.keys(Some(pattern.as_bytes()), None);
        all.sort();
        all
    }

    /// Write or remove markers on up to `limit` rows from `offset`, only
    /// where they disagree with the row's own columns. A second pass over
    /// the same rows changes none.
    pub fn rewrite_threaduser_markers(
        &self,
        offset: usize,
        limit: usize,
    ) -> io::Result<MarkerRewrite> {
        let all = self.thread_user_keys();
        let mut out = MarkerRewrite {
            total_rows: all.len(),
            walked: 0,
            changed: 0,
            already_correct: 0,
        };
        for key in all.iter().skip(offset).take(limit) {
            out.walked += 1;
            let changed = self.store.atomic(|ctx| {
                let fields: HashMap<Vec<u8>, Vec<u8>> = ctx.hgetall(key)?.into_iter().collect();
                let w = marker_repairs(&fields);
                if w.is_empty() {
                    return Ok(false);
                }
                if !w.set.is_empty() {
                    let pairs: Vec<(&[u8], &[u8])> =
                        w.set.iter().map(|(m, v)| (*m, v.as_slice())).collect();
                    ctx.hset(key, &pairs)?;
                }
                if !w.clear.is_empty() {
                    ctx.hdel(key, &w.clear)?;
                }
                Ok(true)
            })?;
            match changed {
                true => out.changed += 1,
                false => out.already_correct += 1,
            }
        }
        Ok(out)
    }

    /// For each marked column: the rows with the column non-zero against
    /// the rows carrying the marker. Read-only. Returns the rows walked and
    /// one entry per column.
    pub fn threaduser_marker_check(&self) -> io::Result<(usize, Vec<MarkerColumnCheck>)> {
        let mut checks: Vec<MarkerColumnCheck> = MARKED
            .iter()
            .map(|m| MarkerColumnCheck {
                column: m.column,
                rows_set: 0,
                markers_present: 0,
                index_entries: 0,
                mismatches: 0,
                samples: Vec::new(),
            })
            .collect();
        let all = self.thread_user_keys();
        for key in &all {
            let fields: HashMap<Vec<u8>, Vec<u8>> = self.store.hgetall(key)?.into_iter().collect();
            for (m, c) in MARKED.iter().zip(checks.iter_mut()) {
                let source = fields.get(m.column.as_bytes()).filter(|v| is_set(v));
                let marker = fields.get(m.marker.as_bytes());
                c.rows_set += usize::from(source.is_some());
                c.markers_present += usize::from(marker.is_some());
                if source != marker {
                    c.mismatches += 1;
                    if c.samples.len() < SAMPLES {
                        c.samples.push(String::from_utf8_lossy(key).into_owned());
                    }
                }
            }
        }
        for (m, c) in MARKED.iter().zip(checks.iter_mut()) {
            let index = format!("threaduser.{}", m.marker);
            c.index_entries = self.store.idx_count(
                index.as_bytes(),
                &kevy_embedded::IndexValue::I64(i64::MIN),
                &kevy_embedded::IndexValue::I64(i64::MAX),
            )?;
        }
        Ok((all.len(), checks))
    }
}
