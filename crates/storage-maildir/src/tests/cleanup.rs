//! `cleanup` — what it removes and what it leaves alone.

use std::fs;
use std::time::{Duration, SystemTime};

use super::tmpdir;
use crate::Maildir;

// --- cleanup ---

#[test]
fn cleanup_old_tmp() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let tmp_dir = tmp.path().join("mail/tmp");

    // create an "old" file by setting its mtime to 48 hours ago
    let old_file = tmp_dir.join("old_file");
    fs::write(&old_file, b"old").unwrap();
    let old_time = SystemTime::now() - Duration::from_secs(48 * 3600);
    filetime::set_file_mtime(&old_file, filetime::FileTime::from_system_time(old_time)).unwrap();

    // create a "new" file
    let new_file = tmp_dir.join("new_file");
    fs::write(&new_file, b"new").unwrap();

    let cleaned = md.cleanup_tmp(Duration::from_secs(36 * 3600)).unwrap();
    assert_eq!(cleaned, 1);
    assert!(!old_file.exists(), "old file should be deleted");
    assert!(new_file.exists(), "new file should be preserved");
}

#[test]
fn cleanup_tmp_empty_dir() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cleaned = md.cleanup_tmp(Duration::from_secs(3600)).unwrap();
    assert_eq!(cleaned, 0);
}

#[test]
fn cleanup_tmp_no_old_files() {
    // all files are fresh — nothing should be removed
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let tmp_dir = tmp.path().join("mail/tmp");

    fs::write(tmp_dir.join("fresh"), b"data").unwrap();

    let cleaned = md.cleanup_tmp(Duration::from_secs(3600)).unwrap();
    assert_eq!(cleaned, 0);
    assert!(tmp_dir.join("fresh").exists());
}

#[test]
fn cleanup_tmp_multiple_old_files() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let tmp_dir = tmp.path().join("mail/tmp");
    let old_time = SystemTime::now() - Duration::from_secs(48 * 3600);

    for name in ["a", "b", "c"] {
        let f = tmp_dir.join(name);
        fs::write(&f, b"x").unwrap();
        filetime::set_file_mtime(&f, filetime::FileTime::from_system_time(old_time)).unwrap();
    }

    let cleaned = md.cleanup_tmp(Duration::from_secs(36 * 3600)).unwrap();
    assert_eq!(cleaned, 3);
}
