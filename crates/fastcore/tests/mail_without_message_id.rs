//! Mail with no `Message-ID` header is indexed under an id taken from its
//! file name, and every path that indexes it arrives at the same id.
//!
//! Before 2026-10-19 delivery skipped such mail and left it to the
//! self-heal sweep, which skipped it as well: it sat in the maildir and on
//! IMAP and never reached the web.

use std::path::Path;
use std::sync::Arc;

const DOMAIN: &str = "x.com";
const USER: &str = "bob@x.com";
const OLD: &str = "1781066005.M390270P1Q0.oldhost";
const OLD_ID: &str = "1781066005.1781066005.M390270P1Q0.oldhost@mailrs.local";

fn letter(subject: &str) -> String {
    format!(
        "From: alerts@monitor.example\r\nTo: {USER}\r\nSubject: {subject}\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n"
    )
}

fn threads(state: &mailrs_fastcore::FastcoreState) -> Vec<String> {
    let mut t = state
        .mailbox
        .all_thread_ids_for_user(USER)
        .expect("threads");
    t.sort();
    t
}

fn row(state: &mailrs_fastcore::FastcoreState, tid: &str) -> mailrs_mailbox_kevy::ThreadRow {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("read")
        .expect("row")
}

fn names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// One test per binary: the maildir root comes from the environment.
#[tokio::test]
async fn mail_without_a_message_id_is_indexed_once_under_its_file_name() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("maildir");
    let md = root.join(DOMAIN).join("bob");
    for leaf in ["cur", "new", "tmp", ".Junk/cur", ".Junk/new", ".Junk/tmp"] {
        std::fs::create_dir_all(md.join(leaf)).expect("maildir");
    }
    let incoming = tmp.path().join("spool/incoming/new");
    std::fs::create_dir_all(&incoming).expect("spool");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", DOMAIN);
    }
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox.upsert_account(USER, "{}").expect("account");
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    // A file that only ever reached the maildir, as the 2,835 did.
    std::fs::write(md.join("new").join(OLD), letter("sluggish_button")).expect("write");
    mailrs_fastcore::self_heal_once(&state, USER).await;
    assert_eq!(threads(&state), [OLD_ID]);
    let unread = row(&state, OLD_ID);
    assert_eq!(unread.unread_count, 1, "a file in new/ is unread");
    assert_eq!(unread.category, "inbox");

    // Read, the file moves to cur/ with S; the id does not move with it.
    std::fs::rename(
        md.join("new").join(OLD),
        md.join("cur").join(format!("{OLD}:2,S")),
    )
    .expect("mv");
    mailrs_fastcore::self_heal_once(&state, USER).await;
    assert_eq!(threads(&state), [OLD_ID], "reading it made a second thread");

    // Delivered: indexed at once, under the id the sweep then agrees on.
    let env = mailrs_core::spool::SpoolEnvelope {
        reverse_path: "alerts@monitor.example".into(),
        forward_paths: vec![USER.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
    };
    std::fs::write(
        incoming.join("1790000000.M1P1.spool"),
        mailrs_core::spool::encode_spool_blob(&env, letter("dead_button").as_bytes()),
    )
    .expect("spool");
    assert_eq!(
        mailrs_fastcore::spool_drain::drain_once(&incoming, root.to_str().expect("utf8"), &state).0,
        1
    );
    let delivered = names(&md.join("new"));
    assert_eq!(delivered.len(), 1);
    let delivered_id = format!(
        "{}.{}@mailrs.local",
        delivered[0].split('.').next().expect("epoch"),
        delivered[0]
    );
    let mut want = vec![OLD_ID.to_string(), delivered_id.clone()];
    want.sort();
    assert_eq!(threads(&state), want, "delivery did not index it");
    mailrs_fastcore::self_heal_once(&state, USER).await;
    assert_eq!(
        threads(&state),
        want,
        "the sweep indexed a delivered message twice"
    );
    assert_eq!(row(&state, &delivered_id).subject, "dead_button");

    // Marked read on disk before the sweep first sees it, which is how the
    // old ones were brought in: indexed as read.
    std::fs::write(
        md.join("cur").join("1790000050.M3P3.host:2,S"),
        letter("rage_tap"),
    )
    .expect("read");
    mailrs_fastcore::self_heal_once(&state, USER).await;
    assert_eq!(
        row(&state, "1790000050.1790000050.M3P3.host@mailrs.local").unread_count,
        0
    );

    // Filed into Junk by a rule: rebuilt as spam, as delivery files it.
    std::fs::write(
        md.join(".Junk/new").join("1790000100.M2P2.host"),
        letter("junk"),
    )
    .expect("junk");
    mailrs_fastcore::self_heal_once(&state, USER).await;
    let junk = row(&state, "1790000100.1790000100.M2P2.host@mailrs.local");
    assert_eq!(junk.category, "spam");
}
