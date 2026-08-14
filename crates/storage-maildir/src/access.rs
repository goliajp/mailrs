//! Read-by-id, mark-processed, and delete primitives.
//!
//! These are the consumption-side operations the receiver/core split needs
//! (S3.1): fetch a delivered message by id, move it `new/` → `cur/` once
//! processed, and remove it. They live in their own `impl Maildir` block so
//! `lib.rs` (delivery + scanning + types) stays under the file-size limit.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::{Flag, Maildir, MessageId};

impl Maildir {
    /// Read the raw bytes of the message identified by `id`, searching
    /// `new/` then `cur/`. Returns `Ok(None)` when no file for that id
    /// exists. The single read-by-id entry point: it encapsulates the
    /// `:2,FLAGS` cur-suffix matching so callers never reconstruct a
    /// filename by hand.
    pub fn fetch(&self, id: &MessageId) -> io::Result<Option<Vec<u8>>> {
        let base = base_id(id);
        let new_path = self.root.join("new").join(base);
        if new_path.is_file() {
            return Ok(Some(fs::read(&new_path)?));
        }
        match self.find_in_cur(id)? {
            Some(path) => Ok(Some(fs::read(&path)?)),
            None => Ok(None),
        }
    }

    /// Mark a message processed: move it from `new/` to `cur/` with the
    /// given `flags` in the `:2,FLAGS` suffix. If it is already in `cur/`,
    /// its suffix is updated to `flags`. Returns `NotFound` if no file for
    /// `id` exists in either directory.
    ///
    /// Keeps the keyword bits already on the file.
    ///
    /// The suffix is rebuilt from `flags`, and a caller that knows about
    /// the six standard flags knows nothing about the lowercase letters
    /// beside them — `archived` and `pinned` live there. Dropping them on
    /// a mark-read would be a fact erased by a write about something else,
    /// so they are read off the file and carried through. To *change* them,
    /// see [`mark_processed_with_keywords`](Self::mark_processed_with_keywords).
    pub fn mark_processed(&self, id: &MessageId, flags: &[Flag]) -> io::Result<()> {
        let keywords = self.keywords_of(id)?.unwrap_or_default();
        self.mark_processed_with_keywords(id, flags, &keywords)
    }

    /// The keyword bits on a message's file, or `None` when there is no
    /// file for `id`. A message in `new/` has none by construction.
    pub fn keywords_of(&self, id: &MessageId) -> io::Result<Option<Vec<char>>> {
        let base = base_id(id);
        if self.root.join("new").join(base).is_file() {
            return Ok(Some(Vec::new()));
        }
        let Some(path) = self.find_in_cur(id)? else {
            return Ok(None);
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        Ok(Some(match name.find(':') {
            Some(i) => crate::keywords_of(&name[i..]),
            None => Vec::new(),
        }))
    }

    /// [`mark_processed`](Self::mark_processed) with the keyword bits
    /// stated rather than carried over — the write path for a keyword.
    pub fn mark_processed_with_keywords(
        &self,
        id: &MessageId,
        flags: &[Flag],
        keywords: &[char],
    ) -> io::Result<()> {
        let base = base_id(id);
        let target = self.root.join("cur").join(format!(
            "{}{}",
            base,
            crate::serialize_flags_and_keywords(flags, keywords)
        ));
        let new_path = self.root.join("new").join(base);
        let src = if new_path.is_file() {
            new_path
        } else if let Some(cur) = self.find_in_cur(id)? {
            cur
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("message {} not found", id.0),
            ));
        };
        if src != target {
            fs::rename(&src, &target)?;
        }
        Ok(())
    }

    /// The flags currently on a message's file, or `None` when no file for
    /// `id` exists in either directory.
    ///
    /// The missing half of [`Self::mark_processed`]: setting flags needs to
    /// know which ones are already there, because the caller's own
    /// representation may not cover all of them. A `u32` bitmask, for one,
    /// has no bit for `P` (passed), so replacing a file's flag set from a
    /// bitmask silently drops it. Read, modify the bits you own, write.
    ///
    /// A message still in `new/` has no suffix and therefore no flags —
    /// `Some(vec![])`, which is different from `None`.
    pub fn flags_of(&self, id: &MessageId) -> io::Result<Option<Vec<Flag>>> {
        let base = base_id(id);
        if self.root.join("new").join(base).is_file() {
            return Ok(Some(Vec::new()));
        }
        let Some(path) = self.find_in_cur(id)? else {
            return Ok(None);
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // From the colon, not past it: `parse_flags` strips a leading
        // `":2,"` itself, so handing it the part after the colon makes it
        // find no prefix and report no flags — silently, as an empty set
        // rather than an error.
        Ok(Some(match name.find(':') {
            Some(i) => crate::parse_flags(&name[i..]),
            None => Vec::new(),
        }))
    }

    /// Delete a message by `id` from `new/` or `cur/`. Returns `NotFound`
    /// if it isn't present in either.
    pub fn delete(&self, id: &MessageId) -> io::Result<()> {
        let new_path = self.root.join("new").join(base_id(id));
        if new_path.is_file() {
            return fs::remove_file(&new_path);
        }
        match self.find_in_cur(id)? {
            Some(path) => fs::remove_file(&path),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("message {} not found", id.0),
            )),
        }
    }

    /// Locate a message's file in `cur/` by matching the base id before
    /// the `:` suffix. `Ok(None)` if `cur/` is absent or holds no match.
    fn find_in_cur(&self, id: &MessageId) -> io::Result<Option<PathBuf>> {
        let cur = self.root.join("cur");
        if !cur.is_dir() {
            return Ok(None);
        }
        for entry in fs::read_dir(&cur)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let base = filename.split(':').next().unwrap_or(filename);
            if base == base_id(id) {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }
}

/// The flag-free identity of a message id: everything before the first
/// `:` (the `:2,FLAGS` info section). Callers sometimes hold ids copied
/// from real cur/ filenames — e.g. blob_refs recorded by the pg-dump
/// import carry the `:2,S` suffix verbatim — and the id must keep
/// matching after flags change (a rename), so every lookup compares on
/// the base.
fn base_id(id: &MessageId) -> &str {
    id.0.split(':').next().unwrap_or(&id.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_maildir() -> (tempfile::TempDir, Maildir) {
        let dir = tempfile::tempdir().unwrap();
        let md = Maildir::create(dir.path().join("mail")).unwrap();
        (dir, md)
    }

    #[test]
    fn fetch_matches_id_that_carries_a_flags_suffix() {
        // blob_refs recorded from real cur/ filenames (pg-dump import)
        // carry `:2,S` verbatim — fetch/mark/delete must still resolve.
        let (_d, md) = tmp_maildir();
        let body = b"From: a@x\r\n\r\nbody\r\n";
        let id = md.deliver(body).unwrap();
        md.mark_processed(&id, &[Flag::Seen]).unwrap();

        let suffixed = MessageId(format!("{}:2,S", id.0));
        assert_eq!(md.fetch(&suffixed).unwrap().as_deref(), Some(&body[..]));

        // re-flagging via the suffixed id must not create a double
        // `:2,S:2,S` filename
        md.mark_processed(&suffixed, &[Flag::Seen, Flag::Flagged])
            .unwrap();
        assert_eq!(md.fetch(&id).unwrap().as_deref(), Some(&body[..]));

        md.delete(&suffixed).unwrap();
        assert!(md.fetch(&id).unwrap().is_none());
    }

    #[test]
    fn fetch_reads_from_new_then_cur() {
        let (_d, md) = tmp_maildir();
        let body = b"From: a@x\r\nSubject: hi\r\n\r\nbody\r\n";
        let id = md.deliver(body).unwrap();

        assert_eq!(md.fetch(&id).unwrap().as_deref(), Some(&body[..]));

        // after new -> cur, fetch still finds it
        md.mark_processed(&id, &[Flag::Seen]).unwrap();
        assert_eq!(md.fetch(&id).unwrap().as_deref(), Some(&body[..]));

        // unknown id -> None
        assert!(md.fetch(&MessageId("nope".into())).unwrap().is_none());
    }

    #[test]
    fn mark_processed_moves_new_to_cur_with_flags() {
        let (_d, md) = tmp_maildir();
        let id = md.deliver(b"x").unwrap();

        md.mark_processed(&id, &[Flag::Seen, Flag::Replied])
            .unwrap();

        assert!(md.scan_new().unwrap().is_empty(), "moved out of new/");
        let cur = md.scan_cur().unwrap();
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].id, id);
        assert_eq!(
            cur[0].flags,
            vec![Flag::Replied, Flag::Seen],
            "flags sorted"
        );
    }

    #[test]
    fn delete_removes_from_new_or_cur_and_errors_on_missing() {
        let (_d, md) = tmp_maildir();
        let id1 = md.deliver(b"one").unwrap();
        let id2 = md.deliver(b"two").unwrap();

        md.delete(&id1).unwrap();
        let new = md.scan_new().unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].id, id2);

        md.mark_processed(&id2, &[Flag::Seen]).unwrap();
        md.delete(&id2).unwrap();
        assert!(md.scan_cur().unwrap().is_empty());

        assert_eq!(
            md.delete(&MessageId("nope".into())).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
