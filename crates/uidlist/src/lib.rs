//! The UID ↔ filename map, stored beside the mail.
//!
//! # Why this is a file and not a table
//!
//! A UID is a promise to an IMAP client: *this number will mean this
//! message for as long as `UIDVALIDITY` does not change*. Break it and
//! every client re-downloads the mailbox. So a UID cannot be recomputed
//! from the mail — it is a **fact**, in the sense
//! `common/data-architecture.md` uses the word, and facts do not belong in
//! an index that may be rebuilt.
//!
//! mailrs kept UIDs in the serving lane's database, which is the thing a
//! lane switch replaces: `.claude/two-lane-known-diff.txt` §7 records the
//! consequence as accepted — "uid / mailbox_id / modseq identity — not
//! preserved across a switch by design … IMAP clients resync". This file
//! is what makes that line unnecessary.
//!
//! # The format is Dovecot's
//!
//! Unchanged, because interoperating with it is free and because it has
//! been through twenty years of edge cases:
//!
//! ```text
//! 3 V1786650000 N4213
//! 4212 :1786650987.M1P1.host
//! 4213 :1786650999.M2P1.host
//! ```
//!
//! A header line — format version, `V`alidity, `N`ext — then one record per
//! message: the UID, optional space-separated extension fields, then `:`
//! and the base filename (no `:2,flags` suffix). Dovecot writes extensions
//! like `W<size>` and `G<guid>`; they are read and ignored here rather than
//! rejected, which is what lets a Dovecot-written file be adopted as-is.
//!
//! # Appending is enough
//!
//! [`append`] writes one record and does not touch the header, so the
//! stored `N` goes stale between rewrites. [`read`] therefore derives
//! `uid_next` as `max(header N, highest uid + 1)`, which makes a
//! header-only-stale file indistinguishable from a fresh one to every
//! caller. That is what allows the hot path to be a single `O_APPEND`
//! write with no read-modify-write and no lock.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The file's name inside a mailbox directory.
///
/// Not `dovecot-uidlist`: the file is Dovecot's *format*, but a name that
/// claims to be Dovecot's file invites a Dovecot installation to trust
/// fields mailrs does not write.
pub const FILE_NAME: &str = "mailrs-uidlist";

/// One mailbox's UID map.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UidList {
    /// `UIDVALIDITY`. Changing it tells every client to forget its cache,
    /// so it is written once and never again.
    pub uid_validity: u32,
    /// The next UID to hand out — always greater than every UID present.
    pub uid_next: u32,
    /// `(uid, filename)`, in file order.
    pub entries: Vec<(u32, String)>,
}

impl UidList {
    /// A new, empty list whose validity is the current second.
    ///
    /// Seconds since the epoch is what Dovecot uses, and the only property
    /// required is that a later mailbox at the same path never reuses an
    /// earlier one's number.
    pub fn new() -> Self {
        Self {
            uid_validity: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as u32)
                .unwrap_or(1),
            uid_next: 1,
            entries: Vec::new(),
        }
    }

    /// Parse a uidlist. Returns `None` only when the header is unusable —
    /// an unreadable *record* is skipped, because one bad line must not
    /// cost a mailbox its other ten thousand UIDs.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = String::from_utf8_lossy(bytes);
        let mut lines = text.lines();
        let header = lines.next()?;
        let mut fields = header.split_whitespace();
        // Format version. Only 3 is written; a file claiming something
        // else is not one this understands, and guessing at it would be
        // worse than declining.
        if fields.next()? != "3" {
            return None;
        }
        let mut uid_validity = 0u32;
        let mut uid_next = 0u32;
        for f in fields {
            let (tag, value) = f.split_at(1);
            match tag {
                "V" => uid_validity = value.parse().ok()?,
                "N" => uid_next = value.parse().ok()?,
                // Dovecot's other header tags (G, etc.) — not ours to
                // interpret, and not a reason to reject the file.
                _ => {}
            }
        }

        let mut entries = Vec::new();
        let mut highest = 0u32;
        for line in lines {
            let Some((uid, name)) = parse_record(line) else {
                continue;
            };
            highest = highest.max(uid);
            entries.push((uid, name));
        }

        Some(Self {
            uid_validity,
            // The header is allowed to be behind: `append` does not
            // rewrite it. What must never happen is handing out a UID that
            // is already in the file.
            uid_next: uid_next.max(highest.saturating_add(1)),
            entries,
        })
    }

    /// Serialize, header first.
    pub fn render(&self) -> Vec<u8> {
        let mut out = format!("3 V{} N{}\n", self.uid_validity, self.uid_next).into_bytes();
        for (uid, name) in &self.entries {
            out.extend_from_slice(record_line(*uid, name).as_bytes());
        }
        out
    }

    /// The filename this UID names, if any.
    ///
    /// Searched from the end: [`append`] is the write path, so a name can
    /// appear more than once and the later line is the current answer.
    pub fn filename(&self, uid: u32) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|(u, _)| *u == uid)
            .map(|(_, n)| n.as_str())
    }

    /// The same list with one record per filename — the last one — in UID
    /// order.
    ///
    /// A derivation of the log, so a crash during the rewrite loses
    /// nothing: the un-compacted file is still the truth. `uid_next` is
    /// carried over untouched, because compaction is housekeeping and must
    /// never hand out or retire a UID. Sorted because that is the order
    /// Dovecot's file is in and the order a person reading it expects;
    /// nothing depends on it, since every lookup is by content.
    pub fn compacted(&self) -> Self {
        let mut keep: Vec<(u32, String)> = Vec::with_capacity(self.entries.len());
        for (uid, name) in &self.entries {
            let base = base_name(name);
            if let Some(slot) = keep.iter_mut().find(|(_, n)| base_name(n) == base) {
                *slot = (*uid, name.clone());
            } else {
                keep.push((*uid, name.clone()));
            }
        }
        keep.sort_by_key(|(uid, _)| *uid);
        Self {
            uid_validity: self.uid_validity,
            uid_next: self.uid_next,
            entries: keep,
        }
    }

    /// The UID this filename carries, if any.
    ///
    /// Compared on the base name, so a caller may pass either
    /// `1786650987.M1P1.host` or `1786650987.M1P1.host:2,S` — the flags
    /// change every time a message is read and the UID must not.
    pub fn uid_of(&self, filename: &str) -> Option<u32> {
        let want = base_name(filename);
        // From the end, for the same reason as `filename`.
        self.entries
            .iter()
            .rev()
            .find(|(_, n)| base_name(n) == want)
            .map(|(u, _)| *u)
    }
}

/// `<mailbox>/mailrs-uidlist`.
pub fn path(mailbox: impl AsRef<Path>) -> PathBuf {
    mailbox.as_ref().join(FILE_NAME)
}

/// Read a mailbox's uidlist. `Ok(None)` when the file does not exist yet;
/// an unparseable file is an error rather than a silent empty list, since
/// treating it as empty would reissue UIDs that are already promised.
pub fn read(mailbox: impl AsRef<Path>) -> io::Result<Option<UidList>> {
    let p = path(mailbox);
    let bytes = match fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    UidList::parse(&bytes).map(Some).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a uidlist", p.display()),
        )
    })
}

/// Append one record, creating the file with a fresh header if absent.
///
/// One `O_APPEND` write: concurrent appenders interleave whole lines
/// rather than corrupting each other, which is why the hot path needs no
/// lock. The header's `N` is left alone — see the module docs.
pub fn append(mailbox: impl AsRef<Path>, uid: u32, filename: &str) -> io::Result<()> {
    append_many(mailbox, &[(uid, filename)])
}

/// Append a batch in one open.
///
/// Same semantics as [`append`], and the same file: a backfill has tens of
/// thousands of records for one mailbox and opening per record is the
/// whole cost. An empty batch writes nothing — a mailbox with no UIDs
/// should not acquire a `UIDVALIDITY` it never promised.
pub fn append_many(mailbox: impl AsRef<Path>, records: &[(u32, &str)]) -> io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let p = path(&mailbox);
    if !p.exists() {
        let mut fresh = UidList::new();
        fresh.uid_next = records
            .iter()
            .map(|(uid, _)| uid.saturating_add(1))
            .max()
            .unwrap_or(1);
        write_atomic(&p, &fresh.render())?;
    }
    let mut buf = String::new();
    for (uid, filename) in records {
        buf.push_str(&record_line(*uid, filename));
    }
    let mut f = fs::OpenOptions::new().append(true).open(&p)?;
    f.write_all(buf.as_bytes())
}

/// Replace the file with `list`, atomically.
///
/// Used by compaction and by a rebuild. Writing in place would leave a
/// reader with half a file and no way to tell.
pub fn rewrite(mailbox: impl AsRef<Path>, list: &UidList) -> io::Result<()> {
    write_atomic(&path(&mailbox), &list.render())
}

fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = target.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, target)
}

fn record_line(uid: u32, filename: &str) -> String {
    format!("{uid} :{}\n", base_name(filename))
}

/// `1786650987.M1P1.host:2,S` → `1786650987.M1P1.host`.
fn base_name(filename: &str) -> &str {
    let file = filename.rsplit('/').next().unwrap_or(filename);
    file.split(':').next().unwrap_or(file)
}

/// `<uid> [ext...] :<filename>` — extensions between the UID and the colon
/// are Dovecot's and are skipped.
fn parse_record(line: &str) -> Option<(u32, String)> {
    let (head, name) = line.split_once(" :")?;
    let uid: u32 = head.split_whitespace().next()?.parse().ok()?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((uid, name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"3 V1786650000 N4214\n\
                            4212 :1786650987.M1P1.host\n\
                            4213 :1786650999.M2P1.host\n";

    #[test]
    fn parses_the_header_and_the_records() {
        let l = UidList::parse(SAMPLE).expect("parse");
        assert_eq!((l.uid_validity, l.uid_next), (1_786_650_000, 4214));
        assert_eq!(l.entries.len(), 2);
        assert_eq!(l.filename(4212), Some("1786650987.M1P1.host"));
        assert_eq!(l.uid_of("1786650999.M2P1.host"), Some(4213));
    }

    /// The flags change every time the message is read; the UID must not.
    #[test]
    fn a_lookup_ignores_the_flag_suffix_and_the_directory() {
        let l = UidList::parse(SAMPLE).expect("parse");
        assert_eq!(l.uid_of("1786650987.M1P1.host:2,S"), Some(4212));
        assert_eq!(l.uid_of("cur/1786650987.M1P1.host:2,FS"), Some(4212));
    }

    /// Dovecot writes fields mailrs does not. Rejecting the file would
    /// mean reissuing UIDs it has already promised to clients.
    #[test]
    fn dovecots_extension_fields_are_read_past() {
        let l =
            UidList::parse(b"3 V1 N9 G0e0f\n7 W1234 G8b8c :1786650987.M1P1.host\n").expect("parse");
        assert_eq!(l.uid_of("1786650987.M1P1.host"), Some(7));
    }

    /// One bad line must not cost a mailbox its other ten thousand UIDs.
    #[test]
    fn a_broken_record_is_skipped_and_the_rest_survive() {
        let l = UidList::parse(b"3 V1 N9\nnot a record\n8 :good.host\n").expect("parse");
        assert_eq!(l.entries.len(), 1);
        assert_eq!(l.uid_of("good.host"), Some(8));
    }

    #[test]
    fn a_header_that_is_not_version_three_is_refused() {
        assert!(UidList::parse(b"2 V1 N9\n8 :good.host\n").is_none());
        assert!(UidList::parse(b"").is_none());
    }

    /// `append` does not rewrite the header, so the stored `N` goes stale.
    /// Reading has to correct it, or the next allocation reissues a UID
    /// that is already in the file and two messages answer to one number.
    #[test]
    fn uid_next_is_derived_when_the_header_is_behind() {
        let l = UidList::parse(b"3 V1 N2\n5 :a.host\n9 :b.host\n").expect("parse");
        assert_eq!(l.uid_next, 10);
    }

    /// Appending is the write path, so the same message can appear twice —
    /// a redelivery, or a sweep that ran before the first append landed.
    /// The later line is the current answer, in both directions, and
    /// compaction is what stops the file growing forever.
    #[test]
    fn the_last_record_for_a_name_wins_and_compaction_keeps_it() {
        let l = UidList::parse(b"3 V1 N9\n5 :a.host\n6 :b.host\n7 :a.host\n").expect("parse");
        assert_eq!(l.uid_of("a.host"), Some(7));
        assert_eq!(l.filename(7), Some("a.host"));

        let c = l.compacted();
        assert_eq!(c.entries, vec![(6, "b.host".into()), (7, "a.host".into())]);
        assert_eq!(c.uid_next, l.uid_next, "compaction is not an allocation");
        assert_eq!(c.compacted().entries, c.entries, "and it converges");
    }

    #[test]
    fn round_trips_through_render() {
        let l = UidList::parse(SAMPLE).expect("parse");
        let again = UidList::parse(&l.render()).expect("reparse");
        assert_eq!(l, again);
    }

    #[test]
    fn append_creates_the_file_then_adds_to_it() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(read(tmp.path()).expect("read").is_none());

        append(tmp.path(), 1, "a.host").expect("append");
        append(tmp.path(), 2, "b.host:2,S").expect("append");

        let l = read(tmp.path()).expect("read").expect("present");
        assert_eq!(l.entries.len(), 2);
        assert_eq!(l.uid_of("b.host"), Some(2), "the suffix is not part of it");
        assert_eq!(l.uid_next, 3);
        assert!(l.uid_validity > 0, "a fresh list stamps its validity");
    }

    /// A rebuild replaces the file. It must not be observable half-written.
    /// A backfill has thirty thousand records for one mailbox, and
    /// `append` opens the file per call.
    #[test]
    fn append_many_writes_them_all_in_one_open() {
        let tmp = tempfile::tempdir().expect("tmp");
        append_many(tmp.path(), &[(3, "c.host"), (1, "a.host"), (2, "b.host")]).expect("append");

        let l = read(tmp.path()).expect("read").expect("present");
        assert_eq!(l.entries.len(), 3);
        assert_eq!(l.uid_of("a.host"), Some(1));
        assert_eq!(l.uid_next, 4, "derived from the highest, not the order");

        // And it appends rather than replacing.
        append_many(tmp.path(), &[(9, "z.host")]).expect("append");
        let l = read(tmp.path()).expect("read").expect("present");
        assert_eq!(l.entries.len(), 4);
        assert_eq!(l.uid_of("c.host"), Some(3), "the earlier batch survives");
    }

    #[test]
    fn append_many_of_nothing_does_not_create_a_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        append_many(tmp.path(), &[]).expect("append");
        assert!(
            read(tmp.path()).expect("read").is_none(),
            "an empty batch wrote a header for a mailbox that has no uids"
        );
    }

    #[test]
    fn rewrite_replaces_and_keeps_the_validity_it_is_given() {
        let tmp = tempfile::tempdir().expect("tmp");
        append(tmp.path(), 1, "a.host").expect("append");
        let before = read(tmp.path()).expect("read").expect("present");

        let list = UidList {
            uid_validity: before.uid_validity,
            uid_next: 100,
            entries: vec![(42, "z.host".into())],
        };
        rewrite(tmp.path(), &list).expect("rewrite");

        let after = read(tmp.path()).expect("read").expect("present");
        assert_eq!(after, list, "the file is exactly what was written");
        assert_eq!(
            after.uid_validity, before.uid_validity,
            "validity survives a rebuild — changing it resyncs every client"
        );
        assert!(!path(tmp.path()).with_extension("tmp").exists());
    }

    #[test]
    fn an_unparseable_file_is_an_error_not_an_empty_list() {
        let tmp = tempfile::tempdir().expect("tmp");
        fs::write(path(tmp.path()), b"garbage\n").expect("write");
        assert!(read(tmp.path()).is_err());
    }
}
