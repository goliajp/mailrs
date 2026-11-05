//! What the health probe reads off a spool: quarantined files, and how
//! long the oldest waiting file has waited.

use std::time::{Duration, SystemTime};

use mailrs_intake::spool::{SpoolHealth, health};

#[test]
fn health_counts_quarantined_files_and_the_oldest_wait() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert_eq!(
        health(root),
        SpoolHealth {
            quarantined: 0,
            oldest_incoming_age_seconds: 0,
        },
        "an empty or missing spool"
    );

    let quarantine = root.join("quarantine");
    std::fs::create_dir_all(&quarantine).unwrap();
    std::fs::write(quarantine.join("F1"), b"x").unwrap();
    std::fs::write(quarantine.join("F1.json"), b"{}").unwrap();
    std::fs::write(quarantine.join("F2"), b"x").unwrap();

    let new = root.join("incoming/new");
    std::fs::create_dir_all(&new).unwrap();
    let old = std::fs::File::create(new.join("waiting")).unwrap();
    old.set_modified(SystemTime::now() - Duration::from_secs(600))
        .unwrap();
    std::fs::write(new.join("fresh"), b"x").unwrap();

    let h = health(root);
    assert_eq!(h.quarantined, 2, "the records beside them are not files");
    assert!(
        (600..700).contains(&h.oldest_incoming_age_seconds),
        "{}",
        h.oldest_incoming_age_seconds
    );
}
