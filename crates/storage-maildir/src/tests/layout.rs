//! Directory creation, delivery, and the on-disk filename shape.

use std::collections::HashSet;
use std::fs;

use super::tmpdir;
use crate::Maildir;

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

#[test]
fn open_does_not_create_dirs() {
    let tmp = tmpdir();
    let path = tmp.path().join("mail");
    let _md = Maildir::open(&path);
    // open() must not create any subdirectories
    assert!(!path.join("tmp").exists());
    assert!(!path.join("new").exists());
    assert!(!path.join("cur").exists());
}
// --- delivery ---

#[test]
fn deliver_to_new() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let data = b"Subject: test\r\n\r\nHello\r\n";
    let _id = md.deliver(data).unwrap();

    let entries: Vec<_> = fs::read_dir(tmp.path().join("mail/new")).unwrap().collect();
    assert_eq!(entries.len(), 1);

    let content = fs::read(entries[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(content, data);
}

#[test]
fn deliver_empty_body() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let id = md.deliver(b"").unwrap();

    let new_dir = tmp.path().join("mail/new");
    let content = fs::read(new_dir.join(&id.0)).unwrap();
    assert!(content.is_empty());
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
fn deliver_many_unique_names() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let ids: HashSet<String> = (0..50)
        .map(|i| md.deliver(format!("msg{i}").as_bytes()).unwrap().0)
        .collect();
    assert_eq!(ids.len(), 50, "all 50 delivered IDs must be distinct");
}

#[test]
fn deliver_atomic() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    md.deliver(b"data").unwrap();

    let tmp_entries: Vec<_> = fs::read_dir(tmp.path().join("mail/tmp")).unwrap().collect();
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

#[test]
fn filename_contains_timestamp_parts() {
    // format: {secs}.M{micros}P{pid}Q{seq}.{hostname}
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let id = md.deliver(b"x").unwrap();
    assert!(id.0.contains(".M"), "filename should contain .M<micros>");
    assert!(id.0.contains('P'), "filename should contain P<pid>");
    assert!(id.0.contains('Q'), "filename should contain Q<seq>");
}
