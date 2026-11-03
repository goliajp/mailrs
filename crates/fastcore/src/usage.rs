//! What each account's mail takes on disk: every file under its maildir's
//! INBOX and Maildir++ subfolders, `cur/` and `new/`. The usage counter
//! the quota stage reads is kept by arrivals and removals; this is what it
//! should say, for the backfill that resets it and the route that measures
//! how far it has drifted.

use std::path::Path;

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(iter) = std::fs::read_dir(dir) {
        for e in iter.flatten() {
            if let Ok(md) = e.metadata()
                && md.is_file()
            {
                total += md.len();
            }
        }
    }
    total
}

fn user_bytes(base: &Path) -> u64 {
    let mut total = dir_bytes(&base.join("cur")) + dir_bytes(&base.join("new"));
    if let Ok(iter) = std::fs::read_dir(base) {
        for e in iter.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') || name == "." || name == ".." {
                continue;
            }
            let sub = e.path();
            if sub.join("new").is_dir() {
                total += dir_bytes(&sub.join("cur")) + dir_bytes(&sub.join("new"));
            }
        }
    }
    total
}

/// `(account, bytes)` for every account maildir under `root`, sorted.
/// Dot-directories at the domain level are not domains: `.deleted` holds
/// deleted accounts' mail.
///
/// # Errors
/// `root` could not be read.
pub fn maildir_usage(root: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(root)?.flatten() {
        if !d.file_type().is_ok_and(|t| t.is_dir())
            || d.file_name().to_string_lossy().starts_with('.')
        {
            continue;
        }
        let domain = d.file_name().to_string_lossy().into_owned();
        let Ok(locals) = std::fs::read_dir(d.path()) else {
            continue;
        };
        for l in locals.flatten() {
            if !l.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let local = l.file_name().to_string_lossy().into_owned();
            out.push((
                format!("{local}@{domain}").to_lowercase(),
                user_bytes(&l.path()),
            ));
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_folder_counts_and_deleted_accounts_do_not() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("x.test").join("u");
        for sub in ["cur", "new", "tmp", ".Sent/cur", ".Sent/new", ".Sent/tmp"] {
            std::fs::create_dir_all(home.join(sub)).unwrap();
        }
        std::fs::write(home.join("new/1"), b"abc").unwrap();
        std::fs::write(home.join("cur/2:2,S"), b"de").unwrap();
        std::fs::write(home.join(".Sent/cur/3:2,S"), b"fghij").unwrap();
        std::fs::write(home.join("tmp/4"), b"not mail yet").unwrap();
        std::fs::create_dir_all(tmp.path().join(".deleted/x.test/old/new")).unwrap();
        assert_eq!(
            maildir_usage(tmp.path()).unwrap(),
            [("u@x.test".to_string(), 10)]
        );
    }
}
