use std::sync::Arc;

use axum::extract::{Path, State};
use mailrs_core_api::types::ConversationFilter;

use super::*;

const U: &str = "bob@golia.jp";

fn mail(id: &str) -> Vec<u8> {
    format!(
        "From: a@outside.example\r\nTo: bob@golia.jp\r\nSubject: {id}\r\n\
         Message-ID: <{id}@outside.example>\r\nDate: Tue, 5 Aug 2026 09:00:00 +0900\r\n\r\nhi\r\n"
    )
    .into_bytes()
}

fn inbox_total(state: &Arc<FastcoreState>, trashed: bool) -> usize {
    let f = ConversationFilter {
        limit: 50,
        folder: Some("INBOX".into()),
        trashed,
        ..Default::default()
    };
    state
        .mailbox
        .list_threads_by_activity(U, &crate::routes::reads::threads_filter(&f), 0, 50)
        .unwrap()
        .1
}

fn files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join("golia.jp/bob/new"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// Deleting is moving to the Trash, which can be undone; emptying it and
/// the purge are what delete for good, files and all.
#[tokio::test]
async fn the_trash_keeps_until_emptied_or_due() {
    let maildir = tempfile::tempdir().unwrap();
    let mut state = crate::tests::fresh_state_owned();
    state.maildir_root = maildir.path().to_string_lossy().into_owned();
    let state = Arc::new(state);
    let new = maildir.path().join("golia.jp/bob/new");
    std::fs::create_dir_all(&new).unwrap();
    for id in ["a", "b", "c"] {
        std::fs::write(new.join(format!("{id}.eml")), mail(id)).unwrap();
        crate::ingest::ingest_delivered_file(
            &state,
            U,
            &format!("{id}.eml"),
            &mail(id),
            "INBOX",
            None,
        );
    }
    assert_eq!(inbox_total(&state, false), 3);
    let tid = |id: &str| format!("{id}@outside.example");
    let path = |id: &str| Path((U.to_string(), tid(id)));

    for id in ["a", "b", "c"] {
        trash_thread(State(state.clone()), path(id)).await;
    }
    assert_eq!(inbox_total(&state, false), 0);
    assert_eq!(inbox_total(&state, true), 3, "the Trash lists them");
    assert_eq!(files(maildir.path()), 3, "nothing deleted yet");

    untrash_thread(State(state.clone()), path("a")).await;
    assert_eq!(inbox_total(&state, false), 1, "restored to the Inbox");

    // b went to the Trash long ago; c just now
    state
        .mailbox
        .set_trashed(U, &tid("b"), true, 1_000)
        .unwrap();
    assert_eq!(purge_once(&state, 2_000, 10).await, 1);
    assert_eq!(inbox_total(&state, true), 1);
    assert_eq!(files(maildir.path()), 2, "b's file is gone");
    assert_eq!(
        purge_once(&state, 2_000, 10).await,
        0,
        "nothing more is due"
    );

    let Json(emptied) = empty_trash(State(state.clone()), Path(U.to_string()))
        .await
        .expect("emptied");
    assert_eq!(emptied.affected, 1);
    assert_eq!(inbox_total(&state, true), 0);
    assert_eq!(files(maildir.path()), 1, "only the restored one is left");
    assert_eq!(inbox_total(&state, false), 1);
}

fn setup(ids: &[&str]) -> (tempfile::TempDir, Arc<FastcoreState>) {
    let maildir = tempfile::tempdir().unwrap();
    let mut state = crate::tests::fresh_state_owned();
    state.maildir_root = maildir.path().to_string_lossy().into_owned();
    let state = Arc::new(state);
    let new = maildir.path().join("golia.jp/bob/new");
    std::fs::create_dir_all(&new).unwrap();
    for id in ids {
        std::fs::write(new.join(format!("{id}.eml")), mail(id)).unwrap();
        crate::ingest::ingest_delivered_file(
            &state,
            U,
            &format!("{id}.eml"),
            &mail(id),
            "INBOX",
            None,
        );
    }
    (maildir, state)
}

/// A file that stays on disk is a conversation self-heal files again, so
/// the delete did not happen: the answer is a 500, and emptying the Trash
/// it was in is not reported as done.
///
/// The file cannot be removed because its directory is read-only, which
/// stops everyone but root; run as root, the case cannot be made and the
/// test says so instead of passing on nothing.
#[tokio::test]
async fn a_file_left_on_disk_fails_the_delete() {
    use std::os::unix::fs::PermissionsExt;
    let (maildir, state) = setup(&["d"]);
    let tid = "d@outside.example".to_string();
    trash_thread(State(state.clone()), Path((U.to_string(), tid.clone()))).await;
    let new = maildir.path().join("golia.jp/bob/new");
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = new.join("probe");
    if std::fs::write(&probe, b"x").is_ok() {
        std::fs::remove_file(&probe).unwrap();
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("SKIPPED: running as root, a read-only directory cannot stop an unlink");
        return;
    }

    let emptied = empty_trash(State(state.clone()), Path(U.to_string())).await;
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        emptied.map(|Json(r)| r.affected),
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert!(new.join("d.eml").is_file(), "the file is still there");
}

/// Mail from somebody else brings a conversation out of the Trash, as in
/// Gmail; left there, the purge would delete the new message unread.
#[tokio::test]
async fn a_reply_brings_a_conversation_out_of_the_trash() {
    let (_maildir, state) = setup(&["e"]);
    let tid = "e@outside.example".to_string();
    trash_thread(State(state.clone()), Path((U.to_string(), tid.clone()))).await;
    assert_eq!(inbox_total(&state, false), 0);

    let reply = b"From: a@outside.example\r\nTo: bob@golia.jp\r\nSubject: Re: e\r\n\
Message-ID: <e2@outside.example>\r\nIn-Reply-To: <e@outside.example>\r\n\
Date: Wed, 6 Aug 2026 09:00:00 +0900\r\n\r\nstill there?\r\n";
    crate::ingest::ingest_delivered_file(&state, U, "e2.eml", reply, "INBOX", None);
    assert_eq!(inbox_total(&state, false), 1, "the reply is in the Inbox");
    assert_eq!(inbox_total(&state, true), 0, "and the Trash is empty");

    // the user's own reply leaves it where it is
    trash_thread(State(state.clone()), Path((U.to_string(), tid))).await;
    let own = b"From: bob@golia.jp\r\nTo: a@outside.example\r\nSubject: Re: e\r\n\
Message-ID: <e3@golia.jp>\r\nIn-Reply-To: <e2@outside.example>\r\n\
Date: Wed, 6 Aug 2026 10:00:00 +0900\r\n\r\nyes\r\n";
    crate::ingest::ingest_delivered_file(&state, U, "e3.eml", own, "Sent", None);
    assert_eq!(inbox_total(&state, true), 1, "an own reply restored it");
}
