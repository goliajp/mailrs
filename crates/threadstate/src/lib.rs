//! The per-thread decisions a file name cannot hold.
//!
//! Step 5 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. Maildir
//! flags carry one bit each and keyword bits carry one more, which is
//! enough for *read* and *archived* and not for:
//!
//! - `snoozed_until` — a decision that carries a **timestamp**;
//! - `category` — a classifier's verdict **at a point in time**, which is
//!   a fact rather than a derivation because re-running the classifier is
//!   expensive and the model moves;
//! - `importance_level` / `importance_score` — the same;
//! - `requires_action` — the same.
//!
//! All of them are things that happened, so by the rule in §1 of that RFC
//! they live next to the mail rather than in the index that serves them.
//!
//! # Append-only, newest wins
//!
//! One NDJSON record per change:
//!
//! ```text
//! {"tid":"t-0@x","at":1786650987,"snoozed_until":1786700000}
//! {"tid":"t-0@x","at":1786651020,"category":"notification"}
//! ```
//!
//! A correction is a new record, never an edit — `common/data-architecture.md`
//! again. A record states only the fields it changes, so the two lines
//! above leave the thread snoozed *and* categorised.
//!
//! **File order decides, not `at`.** Appends are ordered by construction,
//! and a wall clock is not: two processes on one mailbox, or one clock
//! stepping backwards, would otherwise let an older decision win. `at` is
//! recorded because a log nobody can date is hard to reason about, and it
//! is never compared.
//!
//! # Compaction
//!
//! [`ThreadState::compacted`] keeps one record per thread holding the
//! resolved fields. It is a derivation of the log, so a crash midway
//! loses nothing: the un-compacted file is still the truth.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file's name inside a mailbox directory.
pub const FILE_NAME: &str = "mailrs-threadstate";

/// One line of the log: what changed about one thread, and when.
///
/// Every field is optional and absent fields are left untouched on replay,
/// which is what makes a record a *change* rather than a snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The thread this is about.
    pub tid: String,
    /// Epoch seconds the decision was taken. Informational — see the
    /// module docs on why ordering does not use it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub at: i64,
    /// Epoch seconds the reader put the thread away until; `0` un-snoozes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
    /// The classifier's verdict: `inbox`, `notification`, `promotion`,
    /// `spam`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// `low` / `normal` / `high`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance_level: Option<String>,
    /// The score behind the level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance_score: Option<f64>,
    /// Whether the thread was judged to need a reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_action: Option<bool>,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl Record {
    /// A record about `tid` taken at `at`, changing nothing yet.
    pub fn new(tid: impl Into<String>, at: i64) -> Self {
        Self {
            tid: tid.into(),
            at,
            ..Default::default()
        }
    }

    /// Whether this record changes anything at all. A record that states
    /// no field is a line that costs a write and says nothing.
    pub fn is_empty(&self) -> bool {
        self.snoozed_until.is_none()
            && self.category.is_none()
            && self.importance_level.is_none()
            && self.importance_score.is_none()
            && self.requires_action.is_none()
    }

    /// Apply this record's stated fields over `onto`, leaving the rest.
    fn apply_to(&self, onto: &mut Record) {
        onto.tid.clone_from(&self.tid);
        onto.at = self.at;
        if self.snoozed_until.is_some() {
            onto.snoozed_until = self.snoozed_until;
        }
        if self.category.is_some() {
            onto.category.clone_from(&self.category);
        }
        if self.importance_level.is_some() {
            onto.importance_level.clone_from(&self.importance_level);
        }
        if self.importance_score.is_some() {
            onto.importance_score = self.importance_score;
        }
        if self.requires_action.is_some() {
            onto.requires_action = self.requires_action;
        }
    }
}

/// A mailbox's log, replayed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadState {
    resolved: BTreeMap<String, Record>,
    /// How many records the log held, which is what says whether
    /// compaction has anything to do.
    pub records: usize,
}

impl ThreadState {
    /// Replay a log. A line that will not parse is skipped: one bad line
    /// must not cost a mailbox every decision in it.
    pub fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let mut resolved: BTreeMap<String, Record> = BTreeMap::new();
        let mut records = 0usize;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(rec) = serde_json::from_str::<Record>(line) else {
                continue;
            };
            if rec.tid.is_empty() {
                continue;
            }
            records += 1;
            let slot = resolved.entry(rec.tid.clone()).or_default();
            rec.apply_to(slot);
        }
        Self { resolved, records }
    }

    /// What the log says about one thread, or `None` if it says nothing.
    pub fn get(&self, tid: &str) -> Option<&Record> {
        self.resolved.get(tid)
    }

    /// Every thread the log mentions, with its resolved fields.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Record)> {
        self.resolved.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many threads the log speaks about.
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    /// Whether the log speaks about nothing.
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    /// One record per thread, holding the resolved fields.
    pub fn compacted(&self) -> Vec<Record> {
        self.resolved.values().cloned().collect()
    }
}

/// `<mailbox>/mailrs-threadstate`.
pub fn path(mailbox: impl AsRef<Path>) -> PathBuf {
    mailbox.as_ref().join(FILE_NAME)
}

/// Replay a mailbox's log. An absent file is an empty state.
pub fn read(mailbox: impl AsRef<Path>) -> io::Result<ThreadState> {
    match fs::read(path(mailbox)) {
        Ok(b) => Ok(ThreadState::parse(&b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ThreadState::default()),
        Err(e) => Err(e),
    }
}

/// Append one record. A record that states nothing is not written.
pub fn append(mailbox: impl AsRef<Path>, record: &Record) -> io::Result<()> {
    append_many(mailbox, std::slice::from_ref(record))
}

/// Append a batch in one open — the shape a backfill needs.
pub fn append_many(mailbox: impl AsRef<Path>, records: &[Record]) -> io::Result<()> {
    let mut buf = String::new();
    for rec in records {
        if rec.tid.is_empty() || rec.is_empty() {
            continue;
        }
        let Ok(line) = serde_json::to_string(rec) else {
            continue;
        };
        buf.push_str(&line);
        buf.push('\n');
    }
    if buf.is_empty() {
        return Ok(());
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path(&mailbox))?;
    f.write_all(buf.as_bytes())
}

/// Replace the log with one record per thread, atomically.
pub fn rewrite(mailbox: impl AsRef<Path>, records: &[Record]) -> io::Result<()> {
    let target = path(&mailbox);
    let tmp = target.with_extension("tmp");
    let mut buf = String::new();
    for rec in records {
        if rec.tid.is_empty() || rec.is_empty() {
            continue;
        }
        if let Ok(line) = serde_json::to_string(rec) {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(buf.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_states_only_what_changed() {
        let log = b"{\"tid\":\"t1\",\"at\":10,\"snoozed_until\":100}\n\
                    {\"tid\":\"t1\",\"at\":20,\"category\":\"notification\"}\n";
        let s = ThreadState::parse(log);
        let r = s.get("t1").expect("t1");
        assert_eq!(r.snoozed_until, Some(100), "the snooze survived");
        assert_eq!(r.category.as_deref(), Some("notification"));
        assert_eq!(s.records, 2);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn the_later_record_wins_on_the_field_it_states() {
        let log = b"{\"tid\":\"t1\",\"category\":\"inbox\"}\n\
                    {\"tid\":\"t1\",\"category\":\"spam\"}\n";
        assert_eq!(
            ThreadState::parse(log)
                .get("t1")
                .and_then(|r| r.category.clone())
                .as_deref(),
            Some("spam")
        );
    }

    /// A wall clock is not an ordering. Two processes on one mailbox, or
    /// one clock stepping back, would otherwise let an older decision win.
    #[test]
    fn file_order_decides_not_the_timestamp() {
        let log = b"{\"tid\":\"t1\",\"at\":99,\"category\":\"inbox\"}\n\
                    {\"tid\":\"t1\",\"at\":1,\"category\":\"spam\"}\n";
        assert_eq!(
            ThreadState::parse(log)
                .get("t1")
                .and_then(|r| r.category.clone())
                .as_deref(),
            Some("spam"),
            "an earlier `at` on a later line was allowed to lose"
        );
    }

    #[test]
    fn a_broken_line_is_skipped_and_the_rest_survive() {
        let log = b"{\"tid\":\"t1\",\"category\":\"inbox\"}\n\
                    not json\n\
                    {\"no_tid\":true}\n\
                    {\"tid\":\"t2\",\"requires_action\":true}\n";
        let s = ThreadState::parse(log);
        assert_eq!(s.len(), 2);
        assert_eq!(s.get("t2").and_then(|r| r.requires_action), Some(true));
        assert_eq!(
            s.records, 2,
            "the unusable lines are not counted as records"
        );
    }

    /// Zero un-snoozes, and has to be distinguishable from "not stated".
    #[test]
    fn zero_is_a_value_and_absent_is_not() {
        let log = b"{\"tid\":\"t1\",\"snoozed_until\":100}\n\
                    {\"tid\":\"t1\",\"category\":\"inbox\"}\n\
                    {\"tid\":\"t2\",\"snoozed_until\":100}\n\
                    {\"tid\":\"t2\",\"snoozed_until\":0}\n";
        let s = ThreadState::parse(log);
        assert_eq!(
            s.get("t1").and_then(|r| r.snoozed_until),
            Some(100),
            "a record about the category cleared the snooze"
        );
        assert_eq!(
            s.get("t2").and_then(|r| r.snoozed_until),
            Some(0),
            "an explicit zero must un-snooze"
        );
    }

    #[test]
    fn compaction_keeps_the_resolved_fields_and_converges() {
        let log = b"{\"tid\":\"t1\",\"snoozed_until\":100}\n\
                    {\"tid\":\"t1\",\"category\":\"spam\"}\n\
                    {\"tid\":\"t2\",\"importance_level\":\"high\",\"importance_score\":0.9}\n";
        let s = ThreadState::parse(log);
        let c = s.compacted();
        assert_eq!(c.len(), 2);

        let again = ThreadState::parse(
            &c.iter()
                .map(|r| serde_json::to_string(r).expect("json") + "\n")
                .collect::<String>()
                .into_bytes(),
        );
        assert_eq!(again.resolved, s.resolved, "compaction changed the answer");
        assert_eq!(again.records, 2, "and it is one record per thread");
    }

    #[test]
    fn round_trips_through_a_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(read(tmp.path()).expect("read").is_empty());

        let mut r = Record::new("t1", 10);
        r.snoozed_until = Some(100);
        append(tmp.path(), &r).expect("append");
        let mut r2 = Record::new("t1", 20);
        r2.category = Some("spam".into());
        append(tmp.path(), &r2).expect("append");

        let s = read(tmp.path()).expect("read");
        assert_eq!(s.get("t1").and_then(|r| r.snoozed_until), Some(100));
        assert_eq!(
            s.get("t1").and_then(|r| r.category.clone()).as_deref(),
            Some("spam")
        );

        rewrite(tmp.path(), &s.compacted()).expect("rewrite");
        let after = read(tmp.path()).expect("read");
        assert_eq!(after.resolved, s.resolved);
        assert_eq!(after.records, 1, "compacted on disk");
        assert!(!path(tmp.path()).with_extension("tmp").exists());
    }

    /// A line that costs a write and says nothing.
    #[test]
    fn a_record_that_states_nothing_is_not_written() {
        let tmp = tempfile::tempdir().expect("tmp");
        append(tmp.path(), &Record::new("t1", 10)).expect("append");
        assert!(
            read(tmp.path()).expect("read").is_empty(),
            "an empty record was logged"
        );
        assert!(
            !path(tmp.path()).exists(),
            "and it created the file to do it"
        );
    }
}
