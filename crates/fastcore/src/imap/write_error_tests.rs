//! A write the store refused is answered NO, not reported as done.

use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_mailbox_kevy::KevyMailboxStore;

use super::backend::{self, MailboxInfo};
use super::fetch::{expunge, store_response};
use super::session::State;
use crate::FastcoreState;

const USER: &str = "u@x.y";

fn selected(dir: &std::path::Path, names: &[&str]) -> (Arc<FastcoreState>, State) {
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(leaf)).unwrap();
    }
    for (i, name) in names.iter().enumerate() {
        let raw = format!("Message-ID: <m{i}@test>\r\nSubject: t\r\n\r\nbody");
        std::fs::write(dir.join(name), raw).unwrap();
    }
    let state = Arc::new(FastcoreState::new(KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).unwrap(),
    ))));
    let mailbox = MailboxInfo {
        name: "INBOX".into(),
        path: dir.to_path_buf(),
    };
    let messages = backend::list_messages(&state, USER, &mailbox);
    let session = State::Selected {
        user: USER.into(),
        mailbox,
        messages,
        read_only: false,
    };
    (state, session)
}

#[test]
fn a_store_whose_file_is_gone_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, mut session) = selected(tmp.path(), &["new/1700000001.M1P1.h"]);
    std::fs::remove_file(tmp.path().join("new/1700000001.M1P1.h")).unwrap();

    let out = store_response(&state, &mut session, "a1", "1", "+FLAGS", "(\\Seen)", false);

    assert!(
        out.last().unwrap().starts_with("a1 NO STORE failed"),
        "{out:?}"
    );
    assert!(!out.iter().any(|l| l.contains("FETCH")), "{out:?}");
}

#[test]
fn an_expunge_that_cannot_delete_is_refused_and_not_announced() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, mut session) = selected(
        tmp.path(),
        &["cur/1700000001.M1P1.h:2,T", "cur/1700000002.M2P1.h:2,T"],
    );
    // a directory in the file's place: unlink fails, for root as well
    let stuck = tmp.path().join("cur/1700000002.M2P1.h:2,T");
    std::fs::remove_file(&stuck).unwrap();
    std::fs::create_dir(&stuck).unwrap();

    let out = expunge(&state, &mut session, "a1", false);

    // highest seqno first: the stuck one is tried first and stops the rest
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].starts_with("a1 NO EXPUNGE failed"), "{out:?}");
    assert!(tmp.path().join("cur/1700000001.M1P1.h:2,T").exists());
}

#[test]
fn an_expunge_of_a_file_already_gone_is_announced() {
    let tmp = tempfile::tempdir().unwrap();
    let (state, mut session) = selected(tmp.path(), &["cur/1700000001.M1P1.h:2,T"]);
    std::fs::remove_file(tmp.path().join("cur/1700000001.M1P1.h:2,T")).unwrap();

    let out = expunge(&state, &mut session, "a1", false);

    assert_eq!(out, ["* 1 EXPUNGE\r\n", "a1 OK EXPUNGE completed\r\n"]);
}
