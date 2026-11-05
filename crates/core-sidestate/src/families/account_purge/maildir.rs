//! A deleted account's maildir, parked where delivery cannot find it and
//! an admin can move it back, until the purge removes it.
//!
//! ```text
//! <root>/<domain>/<local>                 the live maildir
//! <root>/.deleted/<domain>/<local>.<ts>   parked by the delete at <ts>
//! ```

use std::io;
use std::path::{Path, PathBuf};

/// Move `address`'s maildir to `<root>/.deleted/<domain>/<local>.<ts>`.
/// No maildir is nothing to do.
pub fn park(root: &Path, address: &str, ts: i64) -> io::Result<()> {
    let Some((local, domain)) = address.split_once('@') else {
        return Ok(());
    };
    let from = root.join(domain).join(local);
    match std::fs::symlink_metadata(&from) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    let into = root.join(".deleted").join(domain);
    std::fs::create_dir_all(&into)?;
    std::fs::rename(&from, into.join(format!("{local}.{ts}")))?;
    tracing::info!(%address, to = %into.display(), "deleted account's maildir set aside");
    Ok(())
}

/// Every parked maildir of `address`, oldest first: one per delete, so an
/// address deleted, created again and deleted has two.
pub fn parked(root: &Path, address: &str) -> io::Result<Vec<PathBuf>> {
    let Some((local, domain)) = address.split_once('@') else {
        return Ok(Vec::new());
    };
    let dir = root.join(".deleted").join(domain);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for e in entries {
        let e = e?;
        let name = e.file_name();
        let Some(ts) = name
            .to_str()
            .and_then(|n| n.strip_prefix(local))
            .and_then(|rest| rest.strip_prefix('.'))
        else {
            continue;
        };
        // `<local>.<ts>` only: `a.b.<ts>` is not `a`'s
        if !ts.is_empty() && ts.bytes().all(|b| b.is_ascii_digit()) {
            out.push(e.path());
        }
    }
    out.sort();
    Ok(out)
}

/// Files and bytes under `dirs`.
pub fn measure(dirs: &[PathBuf]) -> io::Result<(u64, u64)> {
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let meta = e.metadata()?;
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    Ok((files, bytes))
}

/// Remove `dirs` and everything in them.
pub fn remove(dirs: &[PathBuf]) -> io::Result<()> {
    for d in dirs {
        std::fs::remove_dir_all(d)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, bytes: usize) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn parking_moves_the_maildir_and_the_purge_finds_only_that_address() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        file(&root.join("x.test/a/cur"), "1:2,S", 10);
        file(&root.join("x.test/a/.Sent/new"), "2", 5);
        file(&root.join(".deleted/x.test/a.b.7"), "other", 1);
        file(&root.join(".deleted/x.test/ab.7"), "other", 1);
        park(root, "a@x.test", 100).unwrap();
        assert!(!root.join("x.test/a").exists());
        // a second account of the same name, deleted later
        file(&root.join("x.test/a/new"), "3", 2);
        park(root, "a@x.test", 200).unwrap();

        let dirs = parked(root, "a@x.test").unwrap();
        assert_eq!(
            dirs,
            [
                root.join(".deleted/x.test/a.100"),
                root.join(".deleted/x.test/a.200")
            ]
        );
        assert_eq!(measure(&dirs).unwrap(), (3, 17));
        remove(&dirs).unwrap();
        assert!(parked(root, "a@x.test").unwrap().is_empty());
        assert!(root.join(".deleted/x.test/a.b.7").exists());
        assert!(root.join(".deleted/x.test/ab.7").exists());
    }

    #[test]
    fn no_maildir_parks_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        park(tmp.path(), "a@x.test", 1).unwrap();
        assert!(parked(tmp.path(), "a@x.test").unwrap().is_empty());
    }
}
