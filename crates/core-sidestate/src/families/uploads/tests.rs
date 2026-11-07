use std::time::{Duration, SystemTime};

use kevy_client::Connection;
use mailrs_core_api::method::admin::{DraftAttachment, DraftWire};

use super::sweep::{self, BASE_INTERVAL, MAX_INTERVAL, Pass};
use super::*;
use crate::families::draft_store;

const ALICE: &str = "alice@x.test";
const BOB: &str = "bob@x.test";

fn stored(root: &Path, user: &str, bytes: &[u8]) -> String {
    match put(root, user, bytes, SystemTime::now()).unwrap() {
        Put::Stored(sha) => sha,
        Put::Full => panic!("full"),
    }
}

fn age(root: &Path, user: &str, sha: &str) {
    let old = SystemTime::now() - KEEP_FOR - Duration::from_secs(5);
    touch(&account_dir(root, user).join(sha), old).unwrap();
}

fn draft_naming(c: &mut Connection, user: &str, id: i64, sha: &str) {
    let d = DraftWire {
        id,
        to: String::new(),
        cc: String::new(),
        bcc: String::new(),
        subject: String::new(),
        body: String::new(),
        reply_to_thread_id: None,
        from: String::new(),
        scheduled_at: None,
        withheld_send_at: None,
        attachments_not_included: Vec::new(),
        attachments: vec![DraftAttachment {
            blob_id: blob_id(sha),
            name: "f".into(),
            content_type: "text/plain".into(),
            size: 1,
        }],
        created_at: 1,
        updated_at: 1,
    };
    draft_store::put(c, user, &d).unwrap();
    // the counter beside the hash is not a draft
    c.incr(crate::families::prefs::drafts_counter_key(user).as_bytes())
        .unwrap();
}

#[test]
fn the_sweep_deletes_what_no_draft_of_the_account_names() {
    let mut c = Connection::connect("mem://uploads-sweep").unwrap();
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let loose = stored(root, ALICE, b"nobody's");
    let named = stored(root, ALICE, b"in a draft");
    let fresh = stored(root, ALICE, b"just uploaded");
    // bob's draft names the same bytes, in bob's own directory
    let elsewhere = stored(root, ALICE, b"in bob's draft");
    stored(root, BOB, b"in bob's draft");
    for sha in [&loose, &named, &elsewhere] {
        age(root, ALICE, sha);
    }
    draft_naming(&mut c, ALICE, 1, &named);
    draft_naming(&mut c, BOB, 1, &elsewhere);

    // the health figure, before a pass: it can be other than zero
    let before = sweep::sweep(&mut c, root, SystemTime::now(), true).unwrap();
    assert_eq!(before.expired_unreferenced, 2);
    assert_eq!(before.deleted, 0);
    assert!(
        account_dir(root, ALICE).join(&loose).exists(),
        "a dry run deletes nothing"
    );

    let pass = sweep::sweep(&mut c, root, SystemTime::now(), false).unwrap();
    assert_eq!(pass.deleted, 2);
    assert_eq!(pass.expired_referenced, 1);
    let dir = account_dir(root, ALICE);
    assert!(!dir.join(&loose).exists());
    assert!(
        !dir.join(&elsewhere).exists(),
        "another account's draft does not count"
    );
    assert!(dir.join(&named).exists());
    assert!(dir.join(&fresh).exists());

    let again = sweep::sweep(&mut c, root, SystemTime::now(), false).unwrap();
    assert_eq!(again.deleted, 0, "nothing more to do");
    assert!(!again.did_work());
    let after = sweep::sweep(&mut c, root, SystemTime::now(), true).unwrap();
    assert_eq!(after.expired_unreferenced, 0);
}

#[test]
fn an_emptied_account_directory_goes() {
    let mut c = Connection::connect("mem://uploads-empty").unwrap();
    let root = tempfile::tempdir().unwrap();
    let sha = stored(root.path(), ALICE, b"x");
    age(root.path(), ALICE, &sha);
    sweep::sweep(&mut c, root.path(), SystemTime::now(), false).unwrap();
    assert!(!account_dir(root.path(), ALICE).exists());
}

#[test]
fn an_idle_sweep_backs_off_to_a_day_and_returns_after_work() {
    let idle = Pass::default();
    let worked = Pass {
        deleted: 1,
        ..Pass::default()
    };
    let mut wait = BASE_INTERVAL;
    let mut waits = Vec::new();
    for _ in 0..7 {
        wait = sweep::next_interval(wait, &idle);
        waits.push(wait.as_secs() / 3600);
    }
    assert_eq!(waits, vec![2, 4, 8, 16, 24, 24, 24]);
    assert_eq!(sweep::next_interval(MAX_INTERVAL, &worked), BASE_INTERVAL);
}

#[test]
fn the_same_bytes_are_one_file_and_an_account_has_a_limit() {
    let root = tempfile::tempdir().unwrap();
    let a = stored(root.path(), ALICE, b"same");
    assert_eq!(stored(root.path(), ALICE, b"same"), a);
    assert_eq!(
        std::fs::read_dir(account_dir(root.path(), ALICE))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        account_dir(root.path(), "Alice@X.test "),
        account_dir(root.path(), ALICE)
    );
    assert_eq!(sha_of(&blob_id(&a)), Some(a.clone()));
    assert_eq!(sha_of(&blob_id(&a.to_ascii_uppercase())), Some(a));
    assert_eq!(sha_of("Uabc"), None);
    assert_eq!(sha_of("B12"), None);
}

/// A full account takes no new upload, but a restore still keeps its files.
#[test]
fn a_full_account_refuses_an_upload_and_a_restore_keeps_its_file() {
    let root = tempfile::tempdir().unwrap();
    let dir = account_dir(root.path(), ALICE);
    std::fs::create_dir_all(&dir).unwrap();
    // sparse: counts toward the limit without taking the disk
    std::fs::File::create(dir.join("filler"))
        .unwrap()
        .set_len(ACCOUNT_LIMIT)
        .unwrap();
    let now = SystemTime::now();
    assert_eq!(
        put(root.path(), ALICE, b"one more", now).unwrap(),
        Put::Full
    );
    let sha = keep(root.path(), ALICE, b"one more", now).unwrap();
    assert_eq!(std::fs::read(dir.join(&sha)).unwrap(), b"one more");
    assert_eq!(
        put(root.path(), ALICE, b"one more", now).unwrap(),
        Put::Stored(sha),
        "what is already there is used again"
    );
}

#[test]
fn the_account_purge_removes_the_directory_and_counts_its_files() {
    let root = tempfile::tempdir().unwrap();
    stored(root.path(), ALICE, b"one");
    stored(root.path(), ALICE, b"two");
    stored(root.path(), BOB, b"bob's");
    assert_eq!(purge_account(root.path(), ALICE, true).unwrap(), 2);
    assert!(account_dir(root.path(), ALICE).exists(), "dry run");
    assert_eq!(purge_account(root.path(), ALICE, false).unwrap(), 2);
    assert!(!account_dir(root.path(), ALICE).exists());
    assert!(account_dir(root.path(), BOB).exists());
    assert_eq!(purge_account(root.path(), ALICE, false).unwrap(), 0);
}
