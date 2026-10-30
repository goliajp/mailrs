//! An archive restores to the files it took, `tmp/` of a maildir aside;
//! its checksum names it; two are kept and nothing else in the target is
//! touched; and a run without room refuses before writing anything.

use std::fs;
use std::io::Write;
use std::path::Path;

use age::secrecy::ExposeSecret;
use mailrs_backup::{Plan, Source, SourceKind};
use sha2::Digest;

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::File::create(path).unwrap().write_all(bytes).unwrap();
}

#[test]
fn an_archive_restores_what_it_took_and_old_ones_go() {
    let data = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let d = data.path();
    write(&d.join("kevy/aof-0.aof"), b"*1\r\n$4\r\nPING\r\n");
    write(
        &d.join("mail/golia.jp/a/new/1"),
        b"Subject: one\r\n\r\nbody",
    );
    write(
        &d.join("mail/golia.jp/a/cur/2:2,S"),
        b"Subject: two\r\n\r\nbody",
    );
    write(&d.join("mail/golia.jp/a/tmp/3"), b"half a message");
    write(&d.join("dkim/golia.jp.pem"), b"-----BEGIN PRIVATE KEY-----");
    // what an operator left in the target long ago
    write(&target.path().join("20260609-pre-cutover/.env"), b"KEEP=me");

    let identity = age::x25519::Identity::generate();
    let plan = Plan {
        sources: vec![
            Source {
                name: "kevy-fastcore".into(),
                path: d.join("kevy"),
                kind: SourceKind::Kevy,
            },
            Source {
                name: "maildir".into(),
                path: d.join("mail"),
                kind: SourceKind::Maildir,
            },
            Source {
                name: "dkim".into(),
                path: d.join("dkim"),
                kind: SourceKind::Files,
            },
        ],
        target: target.path().to_path_buf(),
        recipient: identity.to_public().to_string(),
        keep: 2,
    };
    let roomy = |_: &Path| Ok(u64::MAX);
    let first = mailrs_backup::run(&plan, 1_000, "20261031", roomy).unwrap();
    assert!(first.ok, "{}", first.reason);

    let archive = target.path().join("mailrs-20261031.tar.age");
    let sum = fs::read_to_string(target.path().join("mailrs-20261031.tar.age.sha256")).unwrap();
    let actual = sha2::Sha256::digest(fs::read(&archive).unwrap());
    let actual: String = actual.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(sum, format!("{actual}  mailrs-20261031.tar.age\n"));
    assert_eq!(first.sha256, actual);
    assert!(
        !fs::read(&archive)
            .unwrap()
            .windows(7)
            .any(|w| w == b"Subject"),
        "not encrypted"
    );

    let key = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        key.path(),
        format!("{}\n", identity.to_string().expose_secret()),
    )
    .unwrap();
    let restored = tempfile::tempdir().unwrap();
    let into = restored.path().join("r");
    mailrs_backup::restore(&archive, key.path(), &into).unwrap();
    assert_eq!(
        fs::read(into.join("kevy-fastcore/aof-0.aof")).unwrap(),
        b"*1\r\n$4\r\nPING\r\n"
    );
    assert_eq!(
        fs::read(into.join("maildir/golia.jp/a/cur/2:2,S")).unwrap(),
        b"Subject: two\r\n\r\nbody"
    );
    assert!(into.join("maildir/golia.jp/a/new/1").exists());
    assert!(
        !into.join("maildir/golia.jp/a/tmp").exists(),
        "tmp/ was taken"
    );
    assert!(into.join("dkim/golia.jp.pem").exists());
    assert!(
        mailrs_backup::restore(&archive, key.path(), &into).is_err(),
        "restored over data"
    );

    for day in ["20261101", "20261102"] {
        assert!(mailrs_backup::run(&plan, 2_000, day, roomy).unwrap().ok);
    }
    let mut names: Vec<String> = fs::read_dir(target.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "20260609-pre-cutover",
            "last.json",
            "mailrs-20261101.tar.age",
            "mailrs-20261101.tar.age.sha256",
            "mailrs-20261102.tar.age",
            "mailrs-20261102.tar.age.sha256",
        ]
    );

    // too little room: refused before anything is written
    let tight = |_: &Path| Ok(10);
    let refused = mailrs_backup::run(&plan, 3_000, "20261103", tight).unwrap();
    assert!(!refused.ok);
    assert!(refused.reason.contains("bytes free"), "{}", refused.reason);
    assert!(!target.path().join("mailrs-20261103.tar.age").exists());
    assert!(
        !target
            .path()
            .join(".mailrs-20261103.tar.age.partial")
            .exists()
    );
    let last = mailrs_backup::last_outcome(target.path()).unwrap();
    assert_eq!(last, refused);
    assert!(
        last.last_success.is_some(),
        "the earlier success was forgotten"
    );
}

#[test]
fn room_is_half_again_the_last_archive() {
    assert!(mailrs_backup::enough_space(150, 100));
    assert!(!mailrs_backup::enough_space(149, 100));
}
