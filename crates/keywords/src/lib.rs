//! What a Maildir++ keyword bit stands for.
//!
//! A message's `:2,` suffix carries the six standard flags as uppercase
//! letters and up to 26 **keyword bits** as lowercase ones. The bits mean
//! nothing on their own — the mapping lives in a file beside the mail:
//!
//! ```text
//! 0 archived
//! 1 pinned
//! ```
//!
//! so `1786650987.M1P1.host:2,Sab` is seen, archived and pinned. That is
//! Dovecot's `dovecot-keywords`, unchanged, for the same reason
//! [`mailrs_uidlist`] keeps Dovecot's uidlist format: interoperating is
//! free and the format has already met the edge cases.
//!
//! # Why these facts are here rather than in the index
//!
//! `archived` and `pinned` are a person's decisions and cannot be
//! recomputed from the mail, so by the rule in
//! `.claude/rfcs/20260814-the-maildir-is-the-store.md` they belong next to
//! it. They were per-user index columns, which is the thing a lane switch
//! replaces.
//!
//! # The index is the identity, not the letter
//!
//! A name's **index** is what the file records, and the letter is derived
//! from it (`0` → `a`). Renumbering an existing name would silently
//! re-point every message that carries its bit, so [`Keywords::intern`]
//! only ever appends.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The file's name inside a mailbox directory.
///
/// Not `dovecot-keywords`, for the reason [`mailrs_uidlist`] is not
/// `dovecot-uidlist`: the format is Dovecot's, the file is mailrs's.
pub const FILE_NAME: &str = "mailrs-keywords";

/// The highest keyword index a Maildir++ suffix can express: `a`–`z`.
pub const MAX_INDEX: usize = 25;

/// One mailbox's keyword map.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Keywords {
    /// index → name. Sparse: a file may name 0 and 3 and nothing between.
    by_index: BTreeMap<usize, String>,
}

impl Keywords {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `dovecot-keywords`: `<index> <name>` per line.
    ///
    /// A line that is not that is skipped rather than fatal — one bad line
    /// must not cost a mailbox the meaning of its other bits. A repeated
    /// index keeps the **first** name: later lines are the corrupted ones,
    /// and the first is what the messages on disk were written against.
    pub fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let mut by_index = BTreeMap::new();
        for line in text.lines() {
            let Some((idx, name)) = line.split_once(' ') else {
                continue;
            };
            let Ok(idx) = idx.trim().parse::<usize>() else {
                continue;
            };
            let name = name.trim();
            if idx > MAX_INDEX || name.is_empty() {
                continue;
            }
            by_index.entry(idx).or_insert_with(|| name.to_string());
        }
        Self { by_index }
    }

    /// Serialize, lowest index first.
    pub fn render(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (idx, name) in &self.by_index {
            out.extend_from_slice(format!("{idx} {name}\n").as_bytes());
        }
        out
    }

    /// The letter this name is written as, if the mailbox knows it.
    pub fn letter(&self, name: &str) -> Option<char> {
        self.by_index
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .and_then(|(i, _)| letter_for(*i))
    }

    /// The name this letter stands for, if the mailbox knows it.
    pub fn name(&self, letter: char) -> Option<&str> {
        index_for(letter).and_then(|i| self.by_index.get(&i).map(String::as_str))
    }

    /// The letter for `name`, assigning the lowest free index if it is new.
    ///
    /// Returns `None` only when all 26 are taken. **Appends, never
    /// renumbers**: an index is the identity a message's bit refers to, so
    /// moving one would re-point every message already carrying it.
    pub fn intern(&mut self, name: &str) -> Option<char> {
        if let Some(c) = self.letter(name) {
            return Some(c);
        }
        let free = (0..=MAX_INDEX).find(|i| !self.by_index.contains_key(i))?;
        self.by_index.insert(free, name.to_string());
        letter_for(free)
    }

    /// Every name the given letters stand for, in the order given.
    /// Letters the mailbox has no name for are skipped — a bit whose
    /// meaning is not recorded means nothing, and guessing is worse.
    pub fn names_of(&self, letters: &[char]) -> Vec<&str> {
        letters.iter().filter_map(|c| self.name(*c)).collect()
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }
}

/// `<mailbox>/mailrs-keywords`.
pub fn path(mailbox: impl AsRef<Path>) -> PathBuf {
    mailbox.as_ref().join(FILE_NAME)
}

/// Read a mailbox's keyword map. An absent file is an empty map — unlike a
/// uidlist, nothing is promised by a keyword that has never been set.
pub fn read(mailbox: impl AsRef<Path>) -> io::Result<Keywords> {
    match fs::read(path(mailbox)) {
        Ok(b) => Ok(Keywords::parse(&b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Keywords::new()),
        Err(e) => Err(e),
    }
}

/// Replace the file, atomically. A reader must never see half of it.
pub fn write(mailbox: impl AsRef<Path>, keywords: &Keywords) -> io::Result<()> {
    let target = path(mailbox);
    let tmp = target.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&keywords.render())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &target)
}

fn letter_for(index: usize) -> Option<char> {
    (index <= MAX_INDEX).then(|| (b'a' + index as u8) as char)
}

fn index_for(letter: char) -> Option<usize> {
    letter
        .is_ascii_lowercase()
        .then(|| (letter as u8 - b'a') as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dovecots_file() {
        let k = Keywords::parse(b"0 archived\n1 pinned\n");
        assert_eq!(k.letter("archived"), Some('a'));
        assert_eq!(k.letter("pinned"), Some('b'));
        assert_eq!(k.name('a'), Some("archived"));
        assert_eq!(k.name('b'), Some("pinned"));
        assert_eq!(k.name('c'), None, "an unassigned bit means nothing");
    }

    #[test]
    fn a_sparse_file_keeps_its_numbering() {
        // Dovecot leaves holes when a keyword is removed, and the letter is
        // derived from the index — so `3` is `d` whatever else is present.
        let k = Keywords::parse(b"0 archived\n3 pinned\n");
        assert_eq!(k.letter("pinned"), Some('d'));
        assert_eq!(k.name('b'), None);
    }

    #[test]
    fn a_broken_line_is_skipped_and_the_rest_survive() {
        let k = Keywords::parse(b"0 archived\nnonsense\n\n27 too-high\n1 pinned\n");
        assert_eq!(k.letter("archived"), Some('a'));
        assert_eq!(k.letter("pinned"), Some('b'));
        assert_eq!(k.letter("too-high"), None, "there is no 27th bit");
    }

    /// The messages on disk were written against the first name.
    #[test]
    fn a_repeated_index_keeps_the_first_name() {
        let k = Keywords::parse(b"0 archived\n0 something-else\n");
        assert_eq!(k.name('a'), Some("archived"));
    }

    #[test]
    fn intern_appends_and_is_idempotent() {
        let mut k = Keywords::parse(b"0 archived\n");
        assert_eq!(k.intern("archived"), Some('a'), "already known");
        assert_eq!(k.intern("pinned"), Some('b'));
        assert_eq!(k.intern("pinned"), Some('b'), "twice is the same bit");
        assert_eq!(k.render(), b"0 archived\n1 pinned\n");
    }

    /// Renumbering would re-point every message already carrying the bit.
    #[test]
    fn intern_fills_a_hole_rather_than_renumbering() {
        let mut k = Keywords::parse(b"1 pinned\n");
        assert_eq!(k.intern("archived"), Some('a'), "index 0 was free");
        assert_eq!(
            k.letter("pinned"),
            Some('b'),
            "pinned was renumbered, so every message already carrying its \
             old bit now means something else"
        );
    }

    #[test]
    fn twenty_seven_keywords_do_not_fit() {
        let mut k = Keywords::new();
        for i in 0..=MAX_INDEX {
            assert!(k.intern(&format!("kw{i}")).is_some());
        }
        assert_eq!(k.intern("one-too-many"), None);
    }

    #[test]
    fn names_of_skips_letters_with_no_meaning() {
        let k = Keywords::parse(b"0 archived\n1 pinned\n");
        assert_eq!(k.names_of(&['a', 'z', 'b']), vec!["archived", "pinned"]);
    }

    #[test]
    fn round_trips_through_a_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(
            read(tmp.path()).expect("read").is_empty(),
            "absent is empty"
        );

        let mut k = Keywords::new();
        k.intern("archived");
        k.intern("pinned");
        write(tmp.path(), &k).expect("write");

        assert_eq!(read(tmp.path()).expect("read"), k);
        assert!(!path(tmp.path()).with_extension("tmp").exists());
    }
}
