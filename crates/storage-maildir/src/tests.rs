use std::collections::HashSet;
use std::fs;
use std::time::{Duration, SystemTime};

use crate::{Flag, Maildir, MessageId, add_flag, parse_flags, serialize_flags};

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

// --- Flag::as_char ---

#[test]
fn flag_as_char_all_variants() {
    assert_eq!(Flag::Draft.as_char(), 'D');
    assert_eq!(Flag::Flagged.as_char(), 'F');
    assert_eq!(Flag::Passed.as_char(), 'P');
    assert_eq!(Flag::Replied.as_char(), 'R');
    assert_eq!(Flag::Seen.as_char(), 'S');
    assert_eq!(Flag::Trashed.as_char(), 'T');
}

// --- Flag::from_char ---

#[test]
fn flag_from_char_all_valid() {
    assert_eq!(Flag::from_char('D'), Some(Flag::Draft));
    assert_eq!(Flag::from_char('F'), Some(Flag::Flagged));
    assert_eq!(Flag::from_char('P'), Some(Flag::Passed));
    assert_eq!(Flag::from_char('R'), Some(Flag::Replied));
    assert_eq!(Flag::from_char('S'), Some(Flag::Seen));
    assert_eq!(Flag::from_char('T'), Some(Flag::Trashed));
}

#[test]
fn flag_from_char_unknown_returns_none() {
    for c in ['d', 'f', 'p', 'r', 's', 't', 'X', '1', ' ', '\0'] {
        assert_eq!(Flag::from_char(c), None, "'{c}' should return None");
    }
}

#[test]
fn flag_roundtrip_char() {
    let all = [
        Flag::Draft,
        Flag::Flagged,
        Flag::Passed,
        Flag::Replied,
        Flag::Seen,
        Flag::Trashed,
    ];
    for flag in all {
        assert_eq!(Flag::from_char(flag.as_char()), Some(flag));
    }
}

// --- Flag ordering ---

#[test]
fn flag_ord_matches_char_order() {
    // Maildir spec requires flags to be stored in ASCII order: D < F < P < R < S < T
    assert!(Flag::Draft < Flag::Flagged);
    assert!(Flag::Flagged < Flag::Passed);
    assert!(Flag::Passed < Flag::Replied);
    assert!(Flag::Replied < Flag::Seen);
    assert!(Flag::Seen < Flag::Trashed);
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
fn parse_flags_all_six() {
    let flags = parse_flags(":2,DFPRST");
    assert_eq!(
        flags,
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
fn parse_flags_deduplicates() {
    // duplicate chars in the info string must produce a single flag
    let flags = parse_flags(":2,SSR");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_ignores_unknown_chars() {
    // unknown chars must be silently skipped
    let flags = parse_flags(":2,SXR9");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_no_prefix() {
    // when the info string has no ":2," prefix the result should be empty
    let flags = parse_flags("RS");
    assert!(flags.is_empty(), "without :2, prefix flags must be empty");
}

#[test]
fn parse_flags_wrong_version() {
    // ":1," is not a valid flags section
    let flags = parse_flags(":1,RS");
    assert!(flags.is_empty());
}

#[test]
fn parse_no_flags() {
    let flags = parse_flags(":2,");
    assert!(flags.is_empty());
}

#[test]
fn serialize_flags_sorted() {
    let s = serialize_flags(&[Flag::Seen, Flag::Replied]);
    assert_eq!(s, ":2,RS");
}

#[test]
fn serialize_flags_empty() {
    let s = serialize_flags(&[]);
    assert_eq!(s, ":2,");
}

#[test]
fn serialize_flags_deduplicates() {
    // duplicate input flags must produce a single char each
    let s = serialize_flags(&[Flag::Seen, Flag::Seen, Flag::Draft]);
    assert_eq!(s, ":2,DS");
}

#[test]
fn serialize_flags_all() {
    let s = serialize_flags(&[
        Flag::Trashed,
        Flag::Seen,
        Flag::Replied,
        Flag::Passed,
        Flag::Flagged,
        Flag::Draft,
    ]);
    assert_eq!(s, ":2,DFPRST");
}

#[test]
fn parse_serialize_roundtrip() {
    let original = ":2,DRS";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn add_flag_to_existing() {
    let result = add_flag(":2,S", Flag::Replied);
    assert_eq!(result, ":2,RS");
}

#[test]
fn add_flag_idempotent() {
    // adding a flag that already exists must not duplicate it
    let result = add_flag(":2,RS", Flag::Seen);
    assert_eq!(result, ":2,RS");
}

#[test]
fn add_flag_to_empty_info() {
    // starting from an empty info string
    let result = add_flag(":2,", Flag::Draft);
    assert_eq!(result, ":2,D");
}

#[test]
fn add_flag_to_no_prefix() {
    // info string without ":2," — parse_flags returns [], flag is added fresh
    let result = add_flag("", Flag::Flagged);
    assert_eq!(result, ":2,F");
}

// --- MessageId ---

#[test]
fn message_id_display() {
    let id = MessageId("1234567890.M123P456Q0.host".to_string());
    assert_eq!(id.to_string(), "1234567890.M123P456Q0.host");
}

#[test]
fn message_id_equality() {
    let a = MessageId("abc".to_string());
    let b = MessageId("abc".to_string());
    let c = MessageId("xyz".to_string());
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn message_id_hash_consistency() {
    use std::collections::HashMap;
    let mut map: HashMap<MessageId, u32> = HashMap::new();
    map.insert(MessageId("key".to_string()), 42);
    assert_eq!(map.get(&MessageId("key".to_string())), Some(&42));
    assert_eq!(map.get(&MessageId("other".to_string())), None);
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

// --- additional pure function tests ---

#[test]
fn parse_flags_empty_string() {
    let flags = parse_flags("");
    assert!(flags.is_empty(), "empty string should yield no flags");
}

#[test]
fn parse_flags_colon_only() {
    let flags = parse_flags(":");
    assert!(flags.is_empty());
}

#[test]
fn parse_flags_colon_two_only() {
    let flags = parse_flags(":2");
    assert!(flags.is_empty());
}

#[test]
fn parse_flags_unicode_chars() {
    // unicode characters in the flags section should be ignored
    let flags = parse_flags(":2,S\u{00e9}R\u{00fc}");
    assert_eq!(flags, vec![Flag::Replied, Flag::Seen]);
}

#[test]
fn parse_flags_single_flag() {
    assert_eq!(parse_flags(":2,D"), vec![Flag::Draft]);
    assert_eq!(parse_flags(":2,F"), vec![Flag::Flagged]);
    assert_eq!(parse_flags(":2,P"), vec![Flag::Passed]);
    assert_eq!(parse_flags(":2,R"), vec![Flag::Replied]);
    assert_eq!(parse_flags(":2,S"), vec![Flag::Seen]);
    assert_eq!(parse_flags(":2,T"), vec![Flag::Trashed]);
}

#[test]
fn parse_flags_lowercase_ignored() {
    // lowercase versions of flag chars are not valid
    let flags = parse_flags(":2,dfprst");
    assert!(flags.is_empty());
}

#[test]
fn serialize_flags_single_flag_each() {
    assert_eq!(serialize_flags(&[Flag::Draft]), ":2,D");
    assert_eq!(serialize_flags(&[Flag::Flagged]), ":2,F");
    assert_eq!(serialize_flags(&[Flag::Passed]), ":2,P");
    assert_eq!(serialize_flags(&[Flag::Replied]), ":2,R");
    assert_eq!(serialize_flags(&[Flag::Seen]), ":2,S");
    assert_eq!(serialize_flags(&[Flag::Trashed]), ":2,T");
}

#[test]
fn serialize_flags_reverse_order_normalized() {
    // flags given in reverse order should still serialize in ASCII order
    let s = serialize_flags(&[
        Flag::Trashed,
        Flag::Seen,
        Flag::Replied,
        Flag::Passed,
        Flag::Flagged,
        Flag::Draft,
    ]);
    assert_eq!(s, ":2,DFPRST");
}

#[test]
fn serialize_flags_with_many_duplicates() {
    let s = serialize_flags(&[Flag::Seen, Flag::Seen, Flag::Seen, Flag::Seen]);
    assert_eq!(s, ":2,S");
}

#[test]
fn add_flag_builds_correct_order() {
    // start empty, add flags in reverse order, verify sorted output
    let info = add_flag("", Flag::Trashed);
    let info = add_flag(&info, Flag::Draft);
    let info = add_flag(&info, Flag::Replied);
    assert_eq!(info, ":2,DRT");
}

#[test]
fn add_flag_all_flags_incrementally() {
    let mut info = String::from(":2,");
    for flag in [
        Flag::Seen,
        Flag::Draft,
        Flag::Flagged,
        Flag::Passed,
        Flag::Replied,
        Flag::Trashed,
    ] {
        info = add_flag(&info, flag);
    }
    assert_eq!(info, ":2,DFPRST");
}

#[test]
fn add_flag_idempotent_repeated() {
    // adding the same flag many times should be idempotent
    let mut info = ":2,S".to_string();
    for _ in 0..10 {
        info = add_flag(&info, Flag::Seen);
    }
    assert_eq!(info, ":2,S");
}

#[test]
fn parse_serialize_roundtrip_empty() {
    let original = ":2,";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn parse_serialize_roundtrip_all_flags() {
    let original = ":2,DFPRST";
    let flags = parse_flags(original);
    let serialized = serialize_flags(&flags);
    assert_eq!(serialized, original);
}

#[test]
fn parse_serialize_roundtrip_single_flags() {
    for info in [":2,D", ":2,F", ":2,P", ":2,R", ":2,S", ":2,T"] {
        let flags = parse_flags(info);
        let serialized = serialize_flags(&flags);
        assert_eq!(serialized, info);
    }
}

#[test]
fn flag_clone_preserves_value() {
    let original = Flag::Seen;
    let cloned = original;
    assert_eq!(original, cloned);
}

#[test]
fn flag_debug_format() {
    // verify Debug is implemented and produces expected output
    let debug = format!("{:?}", Flag::Draft);
    assert_eq!(debug, "Draft");
    let debug = format!("{:?}", Flag::Seen);
    assert_eq!(debug, "Seen");
}

#[test]
fn message_id_clone() {
    let original = MessageId("test-id".to_string());
    let cloned = original.clone();
    assert_eq!(original, cloned);
    // verify they are independent copies
    assert_eq!(cloned.0, "test-id");
}

#[test]
fn message_id_debug_format() {
    let id = MessageId("123.M0P1Q2.host".to_string());
    let debug = format!("{:?}", id);
    assert!(debug.contains("123.M0P1Q2.host"));
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
