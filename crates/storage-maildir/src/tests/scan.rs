//! `scan`, including the malformed-name and missing-directory edges.

use std::collections::HashSet;
use std::fs;

use super::tmpdir;
use crate::{Flag, Maildir};

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
fn scan_new_entry_id_matches_filename() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let delivered_id = md.deliver(b"hello").unwrap();

    let entries = md.scan_new().unwrap();
    assert_eq!(entries.len(), 1);
    // the entry id must equal the filename (no flags suffix in new/)
    assert_eq!(entries[0].id, delivered_id);
}

#[test]
fn scan_new_entry_has_no_flags() {
    // messages in new/ have no info suffix so flags must be empty
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    md.deliver(b"msg").unwrap();

    let entries = md.scan_new().unwrap();
    assert!(entries[0].flags.is_empty());
}

#[test]
fn scan_new_entry_path_exists() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    md.deliver(b"content").unwrap();

    let entries = md.scan_new().unwrap();
    assert!(entries[0].path.is_file());
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
fn scan_cur_no_flags_suffix() {
    // a cur/ file without any ":" suffix should parse with empty flags
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cur = tmp.path().join("mail/cur");
    fs::write(cur.join("1234567890.abc.host"), b"msg").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].flags.is_empty());
    assert_eq!(entries[0].id.0, "1234567890.abc.host");
}

#[test]
fn scan_cur_id_strips_info_suffix() {
    // the entry id should contain only the part before ":"
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cur = tmp.path().join("mail/cur");
    fs::write(cur.join("msgid123.host:2,S"), b"data").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(entries[0].id.0, "msgid123.host");
}

#[test]
fn scan_cur_all_flags() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cur = tmp.path().join("mail/cur");
    fs::write(cur.join("id:2,DFPRST"), b"x").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(
        entries[0].flags,
        vec![
            Flag::Draft,
            Flag::Flagged,
            Flag::Passed,
            Flag::Replied,
            Flag::Seen,
            Flag::Trashed,
        ]
    );
}

#[test]
fn scan_empty() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let entries = md.scan_new().unwrap();
    assert!(entries.is_empty());
}

#[test]
fn scan_cur_empty() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let entries = md.scan_cur().unwrap();
    assert!(entries.is_empty());
}
// --- scan edge cases ---

#[test]
fn scan_cur_skips_subdirectories() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cur = tmp.path().join("mail/cur");

    // create a subdirectory inside cur/ - should be skipped
    fs::create_dir(cur.join("subdir")).unwrap();
    // create a regular file
    fs::write(cur.join("msgid:2,S"), b"data").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.0, "msgid");
}

#[test]
fn scan_new_skips_subdirectories() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let new = tmp.path().join("mail/new");

    fs::create_dir(new.join("should-skip")).unwrap();
    fs::write(new.join("realfile"), b"msg").unwrap();

    let entries = md.scan_new().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.0, "realfile");
}

#[test]
fn scan_cur_multiple_files_mixed_flags() {
    let tmp = tmpdir();
    let md = Maildir::create(tmp.path().join("mail")).unwrap();
    let cur = tmp.path().join("mail/cur");

    fs::write(cur.join("msg1:2,S"), b"a").unwrap();
    fs::write(cur.join("msg2:2,RS"), b"b").unwrap();
    fs::write(cur.join("msg3"), b"c").unwrap();
    fs::write(cur.join("msg4:2,DFPRST"), b"d").unwrap();

    let entries = md.scan_cur().unwrap();
    assert_eq!(entries.len(), 4);

    // verify we can find each entry by id
    let ids: HashSet<String> = entries.iter().map(|e| e.id.0.clone()).collect();
    assert!(ids.contains("msg1"));
    assert!(ids.contains("msg2"));
    assert!(ids.contains("msg3"));
    assert!(ids.contains("msg4"));
}

#[test]
fn create_cached_initial_call_creates_dirs() {
    let dir = tmpdir();
    let path = dir.path().join("user-mailbox");
    let _md = Maildir::create_cached(&path).unwrap();
    assert!(path.join("tmp").is_dir());
    assert!(path.join("new").is_dir());
    assert!(path.join("cur").is_dir());
}

#[test]
fn create_cached_repeated_calls_idempotent() {
    let dir = tmpdir();
    let path = dir.path().join("user-mailbox");
    // First call creates dirs.
    let _md1 = Maildir::create_cached(&path).unwrap();
    // Subsequent calls succeed without re-creating (idempotent).
    let md2 = Maildir::create_cached(&path).unwrap();
    let md3 = Maildir::create_cached(&path).unwrap();
    // All should be able to deliver a message.
    md2.deliver(b"From: a@b\r\n\r\n1\r\n").unwrap();
    md3.deliver(b"From: a@b\r\n\r\n2\r\n").unwrap();
    let new_entries = md3.scan_new().unwrap();
    assert_eq!(new_entries.len(), 2);
}

#[test]
fn deliver_batch_empty_no_syscalls() {
    let dir = tmpdir();
    let md = Maildir::create(dir.path()).unwrap();
    let ids = md.deliver_batch(&[]).unwrap();
    assert!(ids.is_empty());
}

#[test]
fn deliver_batch_single_message_equivalent_to_deliver() {
    let dir = tmpdir();
    let md = Maildir::create(dir.path()).unwrap();
    let msgs = [b"From: a@b\r\nSubject: t\r\n\r\nhello\r\n".as_slice()];
    let ids = md.deliver_batch(&msgs).unwrap();
    assert_eq!(ids.len(), 1);
    let entries = md.scan_new().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.0, ids[0].0);
}

#[test]
fn deliver_batch_multiple_messages_all_delivered_in_order() {
    let dir = tmpdir();
    let md = Maildir::create(dir.path()).unwrap();
    let bodies: Vec<Vec<u8>> = (0..10)
        .map(|i| format!("Subject: msg {i}\r\n\r\nbody {i}\r\n").into_bytes())
        .collect();
    let slices: Vec<&[u8]> = bodies.iter().map(|v| v.as_slice()).collect();
    let ids = md.deliver_batch(&slices).unwrap();
    assert_eq!(ids.len(), 10);
    let mut seen = HashSet::new();
    for id in &ids {
        assert!(seen.insert(id.0.clone()), "duplicate id: {}", id.0);
    }
    let entries = md.scan_new().unwrap();
    assert_eq!(entries.len(), 10);
    let tmp_count = fs::read_dir(dir.path().join("tmp")).unwrap().count();
    assert_eq!(tmp_count, 0);
}

#[test]
fn deliver_batch_contents_match_input() {
    let dir = tmpdir();
    let md = Maildir::create(dir.path()).unwrap();
    let bodies: Vec<Vec<u8>> = (0..5)
        .map(|i| format!("body {i}\r\n").into_bytes())
        .collect();
    let slices: Vec<&[u8]> = bodies.iter().map(|v| v.as_slice()).collect();
    let ids = md.deliver_batch(&slices).unwrap();
    for (id, expected) in ids.iter().zip(bodies.iter()) {
        let path = dir.path().join("new").join(&id.0);
        let actual = fs::read(&path).unwrap();
        assert_eq!(&actual, expected, "content mismatch for id {}", id.0);
    }
}

#[test]
fn create_cached_invalidate_then_recreate() {
    let dir = tmpdir();
    let path = dir.path().join("user-mailbox");
    let _md = Maildir::create_cached(&path).unwrap();
    // Wipe the directory off disk to simulate external deletion.
    fs::remove_dir_all(&path).unwrap();
    // Without invalidation, cache still says "ensured" → next
    // create_cached would silently skip mkdir and then deliver()
    // would fail (path doesn't exist). Invalidate to force a real
    // re-create.
    Maildir::invalidate_cache(&path);
    let md = Maildir::create_cached(&path).unwrap();
    md.deliver(b"From: a@b\r\n\r\nok\r\n").unwrap();
    assert!(path.join("tmp").is_dir());
}

/// `locate` is the one resolver `webapi::blob_ref_location` and
/// `fastcore::read_maildir_file` both go through, so a reference that
/// resolves for one resolves for the other by construction. This asserts
/// the property that used to differ between them: a message whose file
/// carries a `:2,FLAGS` suffix is found by its base id.
#[test]
fn locate_then_fetch_finds_a_message_in_either_leaf() {
    let tmp = std::env::temp_dir().join(format!("mailrs-locate-{}", std::process::id()));
    let user_root = tmp.join("x.com").join("bob");
    std::fs::create_dir_all(user_root.join("cur")).unwrap();
    std::fs::create_dir_all(user_root.join("new")).unwrap();
    std::fs::create_dir_all(user_root.join(".Sent").join("cur")).unwrap();

    std::fs::write(user_root.join("new").join("unread.id"), b"in-new").unwrap();
    // Marked Seen: renamed into cur/ with a flag suffix. The form every sent
    // copy takes, and the one a hand-built filename cannot open.
    std::fs::write(user_root.join("cur").join("seen.id:2,S"), b"in-cur-flagged").unwrap();
    std::fs::write(
        user_root.join(".Sent").join("cur").join("sub.id:2,S"),
        b"in-subfolder",
    )
    .unwrap();

    let read = |blob_ref: &str| {
        let (dir, id) = crate::locate(&user_root, blob_ref).expect("locate");
        dir.fetch(&id).expect("fetch")
    };

    assert_eq!(read("unread.id").as_deref(), Some(&b"in-new"[..]));
    assert_eq!(
        read("seen.id").as_deref(),
        Some(&b"in-cur-flagged"[..]),
        "a flagged file must be found by its base id"
    );
    assert_eq!(read(".Sent/sub.id").as_deref(), Some(&b"in-subfolder"[..]));
    assert_eq!(read("absent.id"), None);
    assert!(crate::locate(&user_root, "").is_none());

    let _ = std::fs::remove_dir_all(&tmp);
}
