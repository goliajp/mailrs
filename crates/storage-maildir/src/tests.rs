use std::fs;
use std::time::{Duration, SystemTime};

use crate::{add_flag, parse_flags, serialize_flags, Flag, Maildir};

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

// --- directory initialization ---

#[test]
fn create_dirs() {
    let tmp = tmpdir();
    let path = tmp.path().join("mail");
    let _md = Maildir::create(&path).unwrap();
    assert!(path.join("tmp").is_dir());
    assert!(path.join("new").is_dir());
    assert!(path.join("cur").is_dir());
}

#[test]
fn create_nested() {
    let tmp = tmpdir();
    let path = tmp.path().join("domain").join("user");
    let _md = Maildir::create(&path).unwrap();
    assert!(path.join("tmp").is_dir());
    assert!(path.join("new").is_dir());
    assert!(path.join("cur").is_dir());
}

// --- delivery ---

#[test]
fn deliver_to_new() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let data = b"Subject: test\r\n\r\nHello\r\n";
    let _id = md.deliver(data).unwrap();

    let entries: Vec<_> = fs::read_dir(tmp.path().join("mail/new"))
        .unwrap()
        .collect();
    assert_eq!(entries.len(), 1);

    let content = fs::read(entries[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(content, data);
}

#[test]
fn deliver_unique_names() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let id1 = md.deliver(b"msg1").unwrap();
    let id2 = md.deliver(b"msg2").unwrap();
    assert_ne!(id1, id2);
}

#[test]
fn deliver_atomic() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    md.deliver(b"data").unwrap();

    let tmp_entries: Vec<_> = fs::read_dir(tmp.path().join("mail/tmp"))
        .unwrap()
        .collect();
    assert_eq!(tmp_entries.len(), 0, "tmp/ should be empty after deliver");
}

// --- filename format ---

#[test]
fn filename_no_colon_or_slash() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let id = md.deliver(b"data").unwrap();
    assert!(!id.0.contains(':'), "filename must not contain ':'");
    assert!(!id.0.contains('/'), "filename must not contain '/'");
}

// --- flag operations ---

#[test]
fn parse_flags_rs() {
    let flags = parse_flags(":2,RS");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_order() {
    // input in wrong order, output should be normalized
    let flags = parse_flags(":2,SR");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn serialize_flags_sorted() {
    let s = serialize_flags(&[Flag::Seen, Flag::Replied]);
    assert_eq!(s, ":2,RS");
}

#[test]
fn parse_no_flags() {
    let flags = parse_flags(":2,");
    assert!(flags.is_empty());
}

#[test]
fn add_flag_to_existing() {
    let result = add_flag(":2,S", Flag::Replied);
    assert_eq!(result, ":2,RS");
}

// --- scan ---

#[test]
fn scan_new_entries() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    md.deliver(b"msg1").unwrap();
    md.deliver(b"msg2").unwrap();

    let entries = md.scan_new().unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn scan_cur_with_flags() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();

    // manually place a file in cur/ with flags
    let cur = tmp.path().join("mail/cur");
    fs::write(cur.join("1234567890.abc.host:2,RS"), b"msg").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn scan_empty() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let entries = md.scan_new().unwrap();
    assert!(entries.is_empty());
}

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
    filetime::set_file_mtime(
        &old_file,
        filetime::FileTime::from_system_time(old_time),
    )
    .unwrap();

    // create a "new" file
    let new_file = tmp_dir.join("new_file");
    fs::write(&new_file, b"new").unwrap();

    let cleaned = md.cleanup_tmp(Duration::from_secs(36 * 3600)).unwrap();
    assert_eq!(cleaned, 1);
    assert!(!old_file.exists(), "old file should be deleted");
    assert!(new_file.exists(), "new file should be preserved");
}
