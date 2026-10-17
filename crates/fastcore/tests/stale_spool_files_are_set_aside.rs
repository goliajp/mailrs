//! A spool file nobody can take stays in the spool for a week, then moves
//! to `<spool>/unresolved/` so the drain stops rereading it.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn spool_file(dir: &std::path::Path, name: &str, age: Duration) {
    let env = mailrs_core::spool::SpoolEnvelope {
        reverse_path: "forged@example.org".into(),
        forward_paths: vec!["nobody@aging.example".into()],
        is_authenticated: false,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
    };
    let path = dir.join(name);
    std::fs::write(
        &path,
        mailrs_core::spool::encode_spool_blob(&env, b"Subject: x\r\n\r\nx"),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

#[test]
fn an_undeliverable_file_is_set_aside_after_a_week() {
    let tmp = tempfile::tempdir().unwrap();
    let incoming = tmp.path().join("spool/incoming/new");
    let unresolved = tmp.path().join("spool/unresolved");
    std::fs::create_dir_all(&incoming).unwrap();
    let maildir = tmp.path().join("maildir");
    let day = Duration::from_secs(24 * 3600);
    spool_file(&incoming, "old", 8 * day);
    spool_file(&incoming, "recent", 6 * day);

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
        )),
    ));
    let (delivered, _) =
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state);

    assert_eq!(delivered, 0);
    assert!(unresolved.join("old").is_file());
    assert!(!incoming.join("old").exists());
    assert!(incoming.join("recent").is_file());
    assert!(!unresolved.join("recent").exists());
}
