//! What every [`ThreadStore`] must do, asserted against one: each
//! implementation's tests call [`check_thread_store`] with a store it has
//! seeded, so a store that drifts from the others fails where it is
//! written, not only in a comparison of whole answers.

use super::ThreadStore;

/// `user` must be in `unread_thread`, with something unread in it, and in
/// nothing called `stranger_thread`. Leaves `unread_thread` read,
/// unstarred, unpinned, in the Inbox and not snoozed.
pub async fn check_thread_store<S: ThreadStore>(
    store: &S,
    user: &str,
    unread_thread: &str,
    stranger_thread: &str,
) {
    // a conversation the reader is not in is answered, and not changed
    assert_eq!(
        store.reader_view(user, stranger_thread).await.unwrap(),
        None,
        "a stranger's conversation has no view"
    );
    for (what, member) in [
        ("mark_read", store.mark_read(user, stranger_thread).await),
        (
            "mark_unread",
            store.mark_unread(user, stranger_thread).await,
        ),
        ("star", store.set_starred(user, stranger_thread, true).await),
        ("pin", store.set_pinned(user, stranger_thread, true).await),
        (
            "archive",
            store.set_archived(user, stranger_thread, true).await,
        ),
    ] {
        assert!(!member.unwrap(), "{what} reported a stranger as a member");
    }

    let unread = |label: &str, v: Option<super::ReaderView>| -> bool {
        v.unwrap_or_else(|| panic!("{label}: the reader's conversation has no view"))
            .unread
    };
    let before = store.reader_view(user, unread_thread).await.unwrap();
    assert!(unread("seeded", before.clone()), "the seed is not unread");
    assert!(
        !before.as_ref().unwrap().participants.is_empty(),
        "a view names who is in it"
    );

    assert!(store.mark_read(user, unread_thread).await.unwrap());
    assert!(!unread(
        "after mark_read",
        store.reader_view(user, unread_thread).await.unwrap()
    ));
    assert!(store.mark_unread(user, unread_thread).await.unwrap());
    assert!(unread(
        "after mark_unread",
        store.reader_view(user, unread_thread).await.unwrap()
    ));
    for on in [true, false] {
        assert!(store.set_starred(user, unread_thread, on).await.unwrap());
        assert!(store.set_pinned(user, unread_thread, on).await.unwrap());
        assert!(store.set_archived(user, unread_thread, on).await.unwrap());
    }
    store
        .set_snoozed(user, unread_thread, 4_000_000_000)
        .await
        .unwrap();
    store.set_snoozed(user, unread_thread, 0).await.unwrap();
    assert!(store.mark_read(user, unread_thread).await.unwrap());
    assert_eq!(
        store
            .reader_view(user, unread_thread)
            .await
            .unwrap()
            .map(|v| v.unread),
        Some(false),
        "a reader's acts leave the conversation theirs and read"
    );
}
