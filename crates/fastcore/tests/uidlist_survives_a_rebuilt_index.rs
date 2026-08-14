//! A rebuilt index keeps the UIDs the maildir already promised.
//!
//! Step 3 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. A UID is a
//! promise to an IMAP client — *this number means this message until
//! UIDVALIDITY changes* — so it cannot be recomputed, and until now it
//! lived only in the serving lane's database.
//! `.claude/two-lane-known-diff.txt` §7 records the consequence as
//! accepted: switch lanes and "IMAP clients resync".
//!
//! This drives the real thing: deliver, wipe the index the way a lane
//! switch or a rebuild does, run the self-heal, and read the UID back.

use std::sync::Arc;

const USER: &str = "bob@x.com";

fn seed_maildir(root: &std::path::Path) -> Vec<String> {
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    ["one", "two", "three"]
        .iter()
        .map(|s| {
            let raw = format!(
                "From: a@x.com\r\nTo: {USER}\r\nSubject: {s}\r\n\
                 Message-ID: <m-{s}@x.com>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n"
            );
            md.deliver(raw.as_bytes()).expect("deliver").0
        })
        .collect()
}

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s.upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    s
}

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn a_rebuilt_index_adopts_the_uids_the_maildir_already_names() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let files = seed_maildir(&root);
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
    let mailbox_dir = root.join("x.com").join("bob");

    // ── the first index: the self-heal builds it from the maildir ──
    //
    // Its counter is offset first, and that is the whole discriminating
    // power of this test. A fresh index allocating from 1 would hand the
    // same three messages the same 1, 2, 3 — so with both counters at zero
    // the assertion below passes whether or not anything is adopted, which
    // is how the first version of this test passed with the adoption
    // deliberately disabled. A mailbox that has been served for a while
    // has a counter well past its message count; this is that.
    let first = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    first
        .mailbox
        .register_uid(USER, 5_000, "long-since-expunged@x.com")
        .expect("offset the counter");
    mailrs_fastcore::self_heal_once(&first, USER).await;

    let before: Vec<u32> = files
        .iter()
        .map(|f| uid_of(&first, f).expect("healed message has a uid"))
        .collect();
    assert!(
        before.iter().all(|u| *u > 0),
        "the heal issued no uids: {before:?}"
    );

    // The maildir now names them, which is the half that did not exist.
    let list = mailrs_uidlist::read(&mailbox_dir)
        .expect("read")
        .expect("the heal wrote a uidlist");
    for (f, uid) in files.iter().zip(&before) {
        assert_eq!(
            list.uid_of(f),
            Some(*uid),
            "the maildir does not name {f}, so a rebuild cannot keep its uid"
        );
    }

    // ── the switch: a brand-new index over the same maildir ──
    let second = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&second, USER).await;

    let after: Vec<u32> = files
        .iter()
        .map(|f| uid_of(&second, f).expect("rebuilt message has a uid"))
        .collect();
    assert_eq!(
        after, before,
        "the rebuild issued fresh uids — every IMAP client resyncs"
    );

    // And the next allocation does not collide with what was adopted.
    let fresh = second
        .mailbox
        .allocate_uid(USER, "brand-new@x.com")
        .expect("allocate");
    assert!(
        fresh > *before.iter().max().expect("some"),
        "a rebuilt index handed out {fresh}, which is already promised"
    );
}

/// This user's uid for the message whose file is `filename`, read back the
/// way a caller does: through the per-user row.
fn uid_of(state: &Arc<mailrs_fastcore::FastcoreState>, filename: &str) -> Option<u32> {
    let mid = format!("m-{}@x.com", subject_of(filename)?);
    state
        .mailbox
        .user_message_facts(USER, &mid)
        .ok()
        .flatten()
        .map(|f| f.uid)
}

/// The subject is the only thing distinguishing the three seeded files, and
/// the maildir names them by delivery time, so read it out of the file.
fn subject_of(filename: &str) -> Option<String> {
    let root = std::env::var("MAILRS_MAILDIR").ok()?;
    let dir = std::path::PathBuf::from(root).join("x.com").join("bob");
    let bytes = mailrs_maildir::Maildir::open(&dir)
        .fetch(&mailrs_maildir::MessageId(filename.to_string()))
        .ok()??;
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .find_map(|l| l.strip_prefix("Subject: "))
        .map(|s| s.trim().to_string())
}
