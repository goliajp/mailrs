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

    let Json(emptied) = empty_trash(State(state.clone()), Path(U.to_string())).await;
    assert_eq!(emptied.affected, 1);
    assert_eq!(inbox_total(&state, true), 0);
    assert_eq!(files(maildir.path()), 1, "only the restored one is left");
    assert_eq!(inbox_total(&state, false), 1);
}
