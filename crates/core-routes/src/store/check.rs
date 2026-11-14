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

/// `user` must hold messages in `own_thread` and none in `stranger_thread`.
pub async fn check_message_reads<S: super::MessageReads>(
    store: &S,
    user: &str,
    own_thread: &str,
    stranger_thread: &str,
) {
    let mine = store.thread_messages(user, own_thread).await.unwrap();
    assert!(!mine.is_empty(), "the reader's conversation lists nothing");
    for m in &mine {
        assert_eq!(
            m.thread_id, own_thread,
            "a listed message is in another conversation"
        );
    }
    let first = &mine[0];
    let by_uid = store.message_by_uid(user, first.uid).await.unwrap();
    assert_eq!(
        by_uid.map(|m| m.message_id),
        Some(first.message_id.clone()),
        "the uid of a listed message finds it"
    );
    let by_mid = store
        .message_by_message_id(user, &first.message_id)
        .await
        .unwrap();
    assert_eq!(
        by_mid.map(|m| m.uid),
        Some(first.uid),
        "its Message-ID finds the same copy"
    );
    assert!(
        store
            .thread_messages(user, stranger_thread)
            .await
            .unwrap()
            .is_empty(),
        "a stranger's conversation lists something"
    );
    assert!(
        store
            .message_by_uid(user, u32::MAX)
            .await
            .unwrap()
            .is_none(),
        "a uid the reader never had finds something"
    );
}

/// `user` must be in `unheld_thread`, which is not held and was never
/// released, and in nothing called `stranger_thread`. Releasing held mail
/// is compared across stores with mail that arrived held, which takes the
/// intake to set up.
pub async fn check_hold_store<S: super::HoldStore>(
    store: &S,
    user: &str,
    unheld_thread: &str,
    stranger_thread: &str,
    own_message_id: &str,
) {
    assert!(
        store
            .fraud_verdict(user, own_message_id)
            .await
            .unwrap()
            .is_some(),
        "the reader's own message has no verdict answer"
    );
    assert_eq!(
        store
            .fraud_verdict(user, "<nobody-has-this@nowhere.test>")
            .await
            .unwrap(),
        None,
        "a message the reader never had answers a verdict"
    );
    let (_, held) = store.held_page(user, None, 1000).await.unwrap();
    let (none, same) = store.held_page(user, None, 0).await.unwrap();
    assert!(none.is_empty(), "a page of 0 has rows");
    assert_eq!(same, held, "asking only for the count changes the count");
    assert_eq!(
        store.held(user, stranger_thread).await.unwrap(),
        None,
        "a stranger's conversation reads as theirs"
    );
    assert_eq!(
        store.held(user, unheld_thread).await.unwrap(),
        Some(false),
        "ordinary mail reads as held"
    );
    assert!(
        store
            .releases(user, unheld_thread)
            .await
            .unwrap()
            .is_empty(),
        "mail never held has releases"
    );
}

/// `user` must be in `own_thread` and not in `stranger_thread`, and have no
/// label named "check-label" or "check-label-renamed".
pub async fn check_label_store<S: super::LabelStore>(
    store: &S,
    user: &str,
    own_thread: &str,
    stranger_thread: &str,
) {
    use super::LabelFailure;
    use mailrs_core_api::method::label::{LABEL_COLORS, LabelError};
    let color = LABEL_COLORS[0];
    let label = store
        .create_label(user, "check-label", color, 1)
        .await
        .unwrap();
    assert!(
        matches!(
            store.create_label(user, "check-label", color, 2).await,
            Err(LabelFailure::Refused(LabelError::Taken(_)))
        ),
        "a second label of the same name was made"
    );
    let renamed = store
        .update_label(user, &label.id, Some("check-label-renamed"), None)
        .await
        .unwrap();
    assert_eq!(renamed.name, "check-label-renamed");
    assert!(
        store
            .labels(user)
            .await
            .unwrap()
            .iter()
            .any(|l| l.id == label.id && l.name == "check-label-renamed"),
        "the renamed label is not listed"
    );
    assert!(
        matches!(
            store
                .set_label(user, &label.id, stranger_thread, true)
                .await,
            Err(LabelFailure::Refused(LabelError::NotFound))
        ),
        "a stranger's conversation took the label"
    );
    store
        .set_label(user, &label.id, own_thread, true)
        .await
        .unwrap();
    store
        .set_label(user, &label.id, own_thread, false)
        .await
        .unwrap();
    assert!(store.delete_label(user, &label.id).await.unwrap());
    assert!(
        !store.delete_label(user, &label.id).await.unwrap(),
        "a deleted label was deleted again"
    );
}
