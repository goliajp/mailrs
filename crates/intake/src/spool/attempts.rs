//! What the drain was doing with each spool file, kept on disk so that a
//! process that dies in the middle of one (a parser panic under
//! `panic = "abort"`, an OOM kill, a segfault, a deploy) leaves behind
//! which file it was and how far it got.
//!
//! `<spool>/attempts/<F>` is written, fsynced and renamed into place before
//! anything is done with spool file `F`, and every recipient whose copy was
//! taken is appended to it the same way. A file the drain finishes leaves
//! no record; a file left in the spool for another tick keeps one with its
//! count of interrupted attempts back at 0. So a record that still counts
//! attempts when the drain comes to `F` again says the last process died
//! while holding it, and after [`LIMIT`] such deaths the file is set aside
//! (`quarantine`) instead of taking the drain down a fourth time.
//!
//! A recipient recorded as served is skipped on every later attempt. One
//! that died part way is served again, and the steps of serving it hold
//! their own line: the maildir copy is not written twice
//! (`maildir::deliver`), a core's index takes the same file once, and an
//! outbound job carries a once-key. What a death between a side effect
//! leaving (a push, a webhook) and the recipient being recorded can still
//! repeat, it repeats at most once more: delivery is at least once.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Interrupted attempts before a spool file is quarantined.
///
/// Not 1: a deploy restart or another task's OOM also interrupts whatever
/// file is in hand. A message normally takes milliseconds, so that
/// happening to the same file twice running is unlikely, and quarantine
/// can be undone by moving the file back.
pub const LIMIT: u32 = 3;

/// One spool file's record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Attempts started and not finished.
    pub attempts: u32,
    /// unix seconds of the first and the latest attempt
    pub first_at: i64,
    pub last_at: i64,
    /// The step the latest attempt started last.
    pub stage: String,
    /// Steps already done for this file: `rcpt <recipient>` for each
    /// recipient whose copy was taken, `sent <user>` for the sender's copy.
    pub done: Vec<String>,
}

/// An attempt in progress on one spool file.
pub struct Attempt {
    path: PathBuf,
    /// The record left by an earlier attempt, if there was one.
    pub retry: bool,
    record: Record,
}

/// What to do with a spool file.
pub enum Begin {
    /// Go ahead; the attempt is on disk.
    Go(Attempt),
    /// It was interrupted [`LIMIT`] times; set it aside.
    Quarantine(Record),
}

pub(crate) fn dir(spool_root: &Path) -> PathBuf {
    spool_root.join("attempts")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn read(path: &Path) -> Option<Record> {
    let raw = std::fs::read(path).ok()?;
    match serde_json::from_slice(&raw) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "spool attempt record unreadable; starting afresh");
            None
        }
    }
}

/// Write `value` to `path` so that a crash leaves either the old file or
/// the new one: a temporary file, fsync, rename, fsync of the directory.
pub(crate) fn write_atomic(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("record path has no directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    let mut file = std::fs::File::create(&tmp)?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(parent)?.sync_all()
}

/// Start an attempt on spool file `filename`. A record left in
/// `quarantine/` beside a file moved back for another try supplies the
/// steps already done, so a re-injected file delivers nothing twice.
pub fn begin(spool_root: &Path, filename: &str) -> Begin {
    let path = dir(spool_root).join(filename);
    let earlier = read(&path).or_else(|| {
        crate::spool::quarantine::record_beside(spool_root, filename)
            .map(|r| Record { attempts: 0, ..r })
    });
    let retry = earlier.is_some();
    let mut record = earlier.unwrap_or_default();
    if record.attempts >= LIMIT {
        return Begin::Quarantine(record);
    }
    if record.attempts > 0 {
        tracing::warn!(
            event = "spool_interrupted",
            file = %filename,
            attempts = record.attempts,
            stage = %record.stage,
            "the last process stopped while delivering this spool file"
        );
    }
    let at = now();
    if record.first_at == 0 {
        record.first_at = at;
    }
    record.last_at = at;
    record.attempts += 1;
    record.stage = "start".into();
    let attempt = Attempt {
        path,
        retry,
        record,
    };
    attempt.save();
    Begin::Go(attempt)
}

impl Attempt {
    fn save(&self) {
        if let Err(e) = write_atomic(&self.path, &self.record) {
            tracing::warn!(path = %self.path.display(), error = %e, "spool attempt record not written");
        }
    }

    /// Whether `step` was done by an earlier attempt.
    pub fn is_done(&self, step: &str) -> bool {
        self.record.done.iter().any(|d| d == step)
    }

    /// `step` is starting.
    pub fn start(&mut self, step: &str) {
        self.record.stage = step.to_string();
        self.save();
    }

    /// `step` is done; a later attempt skips it.
    pub fn done(&mut self, step: &str) {
        if !self.is_done(step) {
            self.record.done.push(step.to_string());
        }
        self.save();
    }

    /// The record as it stands.
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// The attempt ended. `file_left` is whether the spool file is still
    /// where it was: then the record stays, with no attempt counted, for
    /// the steps it holds; otherwise it goes, with any copy beside a
    /// re-injected file.
    pub fn finish(mut self, spool_root: &Path, filename: &str, file_left: bool) {
        if file_left {
            self.record.attempts = 0;
            self.record.stage.clear();
            self.save();
            return;
        }
        remove(spool_root, filename);
    }
}

/// Forget spool file `filename`: it left the spool.
pub(crate) fn remove(spool_root: &Path, filename: &str) {
    for path in [
        dir(spool_root).join(filename),
        crate::spool::quarantine::record_path(spool_root, filename),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "spool attempt record not removed")
            }
        }
    }
}
