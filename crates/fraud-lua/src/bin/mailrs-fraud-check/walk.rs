//! Listing a maildir and reading its files while a live server moves them.
//!
//! On a running server a message is renamed `new/` → `cur/` when it is
//! first seen and gains flags (`:2,S`) when it is read, so a file listed
//! at the start of a long run may be somewhere else by the time it is
//! read. Its unique name (before the `:`) does not change: a file that is
//! not where it was listed is looked for again by that name in a fresh
//! listing. One that is in no listing any more was deleted, and is
//! reported as vanished rather than judged.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Fresh listings for one file, or for one walk, before giving up.
const ATTEMPTS: usize = 3;

pub struct Maildir {
    root: PathBuf,
    max: usize,
    current: HashMap<String, PathBuf>,
    /// Listings taken again because a file had moved.
    pub rewalks: u64,
    /// Files listed once and found in no later listing.
    pub vanished: BTreeSet<String>,
}

impl Maildir {
    /// Every message file under `root` (`cur/` and `new/`, not `tmp/`),
    /// sorted; an error past `max` files.
    pub fn list(root: &Path, max: usize) -> Result<(Self, Vec<PathBuf>), String> {
        let mut m = Maildir {
            root: root.to_path_buf(),
            max,
            current: HashMap::new(),
            rewalks: 0,
            vanished: BTreeSet::new(),
        };
        let files = m.relist()?;
        Ok((m, files))
    }

    /// The message `listed` names, its first `limit` bytes, from wherever
    /// it is now. `None` when it has left the maildir; it is then in
    /// [`Self::vanished`].
    pub fn read(
        &mut self,
        listed: &Path,
        limit: u64,
    ) -> Result<Option<(PathBuf, Vec<u8>)>, String> {
        let key = key(&self.root, listed);
        let mut at = self
            .current
            .get(&key)
            .cloned()
            .unwrap_or_else(|| listed.to_path_buf());
        for _ in 0..ATTEMPTS {
            match read_up_to(&at, limit) {
                Ok(bytes) => return Ok(Some((at, bytes))),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", at.display())),
            }
            self.rewalks += 1;
            self.relist()?;
            match self.current.get(&key) {
                Some(now) => at = now.clone(),
                None => break,
            }
        }
        self.vanished.insert(key);
        Ok(None)
    }

    fn relist(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut last = String::new();
        for _ in 0..ATTEMPTS {
            let mut files = Vec::new();
            match walk(&self.root, &mut files, self.max) {
                Ok(()) => {
                    files.sort();
                    self.current = files
                        .iter()
                        .map(|f| (key(&self.root, f), f.clone()))
                        .collect();
                    return Ok(files);
                }
                // a directory or entry went away under the walk: walk again
                Err(e) if e.kind() == io::ErrorKind::NotFound => last = e.to_string(),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(format!("the maildir kept changing under the walk: {last}"))
    }
}

/// A message's identity in the maildir: its path with the `cur`/`new`
/// step and the flags dropped.
fn key(root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(name) = parts.pop() {
        if parts.last().is_some_and(|d| d == "cur" || d == "new") {
            parts.pop();
        }
        parts.push(name.split(':').next().unwrap_or("").to_string());
    }
    parts.join("/")
}

fn read_up_to(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    fs::File::open(path)?.take(limit).read_to_end(&mut out)?;
    Ok(out)
}

fn walk(root: &Path, out: &mut Vec<PathBuf>, max: usize) -> io::Result<()> {
    let too_many = || {
        io::Error::other(format!(
            "maildir exceeds {max} messages; use a larger explicit bound"
        ))
    };
    if out.len() > max {
        return Err(too_many());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() && entry.file_name() != "tmp" {
            walk(&entry.path(), out, max)?;
        } else if ty.is_file() && root.file_name().is_some_and(|s| s == "cur" || s == "new") {
            out.push(entry.path());
        }
        if out.len() > max {
            return Err(too_many());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maildir(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("fraud-check-walk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for d in ["golia.jp/u/new", "golia.jp/u/cur", "golia.jp/u/tmp"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        root
    }

    #[test]
    fn a_file_moved_from_new_to_cur_after_the_listing_is_read_where_it_went() {
        let root = maildir("moved");
        fs::write(
            root.join("golia.jp/u/new/1700.M1P2Q3.host"),
            b"Subject: one\r\n\r\n",
        )
        .unwrap();
        fs::write(
            root.join("golia.jp/u/cur/1600.M1P2Q3.host:2,S"),
            b"Subject: two\r\n\r\n",
        )
        .unwrap();
        let (mut m, files) = Maildir::list(&root, 10).unwrap();
        assert_eq!(files.len(), 2);
        // the server sees it, then the reader opens it
        fs::rename(
            root.join("golia.jp/u/new/1700.M1P2Q3.host"),
            root.join("golia.jp/u/cur/1700.M1P2Q3.host:2,S"),
        )
        .unwrap();
        let mut read = Vec::new();
        for f in &files {
            let (at, bytes) = m.read(f, 1024).unwrap().expect("still there");
            read.push((at, bytes));
        }
        assert_eq!(m.rewalks, 1);
        assert!(m.vanished.is_empty());
        assert!(read.iter().any(|(at, b)| {
            at.ends_with("golia.jp/u/cur/1700.M1P2Q3.host:2,S") && b.starts_with(b"Subject: one")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_deleted_after_the_listing_is_reported_not_skipped() {
        let root = maildir("deleted");
        fs::write(root.join("golia.jp/u/cur/1.M1.host:2,"), b"x").unwrap();
        fs::write(root.join("golia.jp/u/tmp/2.M2.host"), b"in delivery").unwrap();
        let (mut m, files) = Maildir::list(&root, 10).unwrap();
        assert_eq!(files.len(), 1, "tmp/ is not mail yet");
        fs::remove_file(&files[0]).unwrap();
        assert!(m.read(&files[0], 1024).unwrap().is_none());
        assert_eq!(
            m.vanished.iter().collect::<Vec<_>>(),
            ["golia.jp/u/1.M1.host"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_listing_past_the_bound_is_refused() {
        let root = maildir("bound");
        for i in 0..3 {
            fs::write(root.join(format!("golia.jp/u/cur/{i}")), b"x").unwrap();
        }
        assert!(Maildir::list(&root, 2).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
