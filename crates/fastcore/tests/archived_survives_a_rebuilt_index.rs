//! Archiving is a decision, so a rebuilt index must not lose it.
//!
//! Step 4 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`.
//! `archived` and `pinned` are things a person did and cannot be
//! recomputed from the mail, so they belong beside it — as Maildir++
//! keyword bits, with `mailrs-keywords` recording which letter means what.
//! They were index columns, and the index is the thing a lane switch
//! replaces.
//!
//! No standard maildir flag means "archived", which is exactly why a
//! keyword bit is the representation rather than a seventh flag letter.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

const USER: &str = "bob@x.com";

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

async fn post(state: &Arc<mailrs_fastcore::FastcoreState>, uri: String) {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert!(res.status().is_success(), "{}", res.status());
}

fn cur_names(root: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(root.join("x.com").join("bob").join("cur"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn the_archive_bit_is_written_to_the_file_and_read_back_on_a_rebuild() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    md.deliver(
        b"From: a@x.com\r\nTo: bob@x.com\r\nSubject: hi\r\n\
          Message-ID: <m-1@x.com>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n",
    )
    .expect("deliver");
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    // A mailbox with an index built from the maildir, as production has.
    let first = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&first, USER).await;
    let tid = first
        .mailbox
        .all_thread_ids_for_user(USER)
        .expect("threads")
        .into_iter()
        .next()
        .expect("the heal made a thread");
    assert!(
        !first
            .mailbox
            .get_thread_for_user(USER, &tid)
            .expect("row")
            .expect("present")
            .archived
    );

    // The verb.
    post(&first, format!("/v1/users/{USER}/threads/{tid}/archive")).await;

    // Tier 1 carries it: a keyword bit on the file, and a map that says
    // what the bit means. Either alone is unreadable.
    let name = cur_names(&root).into_iter().next().unwrap_or_default();
    let kw = mailrs_keywords::read(&md_dir).expect("keywords");
    let letter = kw.letter("archived").expect("the map names the bit");
    let suffix = name.rsplit_once(":2,").map(|(_, s)| s).unwrap_or_default();
    assert!(
        suffix.contains(letter),
        "the file does not carry the archived bit: {name} (letter {letter})"
    );

    // The switch: a new index over the same maildir.
    let second = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&second, USER).await;
    assert!(
        second
            .mailbox
            .get_thread_for_user(USER, &tid)
            .expect("row")
            .expect("present")
            .archived,
        "the rebuilt index shows the thread in the inbox again — the \
         archive was an index column and the index was replaced"
    );

    // And back: unarchiving clears the bit, or the next rebuild undoes it.
    post(&second, format!("/v1/users/{USER}/threads/{tid}/unarchive")).await;
    let name = cur_names(&root).into_iter().next().unwrap_or_default();
    let suffix = name.rsplit_once(":2,").map(|(_, s)| s).unwrap_or_default();
    assert!(
        !suffix.contains(letter),
        "the archived bit survived unarchiving: {name}"
    );
}
