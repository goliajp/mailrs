use super::*;
use crate::backend::{Directory, Protocol};
use crate::test_support::{Memory, MemoryAccount, PASSWORD, USER, directory};

async fn view(dir: &Memory) -> TrashView<MemoryAccount> {
    let a = dir.login(USER, PASSWORD, Protocol::Imap).await.unwrap();
    TrashView::new(a.unwrap())
}

async fn uids(v: &TrashView<MemoryAccount>, name: &str) -> Vec<u32> {
    let mb = v.mailbox(name).await.unwrap().unwrap();
    v.messages(&mb)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uid)
        .collect()
}

#[tokio::test]
async fn trash_is_listed_and_moving_there_and_back_moves_the_message() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.deliver("Subject: two\r\n\r\n2");
    let v = view(&dir).await;
    assert!(
        v.mailbox_names()
            .await
            .unwrap()
            .contains(&"Trash".to_string())
    );

    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let trash = v.mailbox("trash").await.unwrap().unwrap();
    let before = v.select(&trash).await.unwrap().uidvalidity;
    let first = v.messages(&inbox).await.unwrap()[0].clone();
    let filed = v.move_to(&inbox, &first, &trash).await.unwrap();
    assert_eq!(uids(&v, "INBOX").await, [2]);
    assert_eq!(uids(&v, "Trash").await, [filed.uid]);
    let after = v.select(&trash).await.unwrap();
    assert_ne!(after.uidvalidity, before, "a client would not look again");
    assert_eq!(filed.uidvalidity, after.uidvalidity);
    assert_eq!(after.messages[0].seqno, 1);

    let back = after.messages[0].clone();
    v.move_to(&trash, &back, &inbox).await.unwrap();
    assert_eq!(uids(&v, "INBOX").await, [1, 2]);
    assert!(uids(&v, "Trash").await.is_empty());
}

/// Without MOVE a client deletes with COPY to Trash, `\Deleted` and
/// EXPUNGE; the copy is the same message, so that EXPUNGE must not
/// delete it.
#[tokio::test]
async fn copy_to_trash_then_expunge_keeps_the_message_in_trash() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let trash = v.mailbox("Trash").await.unwrap().unwrap();
    let listed = v.messages(&inbox).await.unwrap();
    v.copy(&inbox, &listed[0], &trash).await.unwrap();
    let gone = v.expunge(&inbox, &listed).await;
    assert!(gone.failed.is_none());
    assert_eq!(gone.removed, [1]);

    let in_trash = v.messages(&trash).await.unwrap();
    assert_eq!(in_trash.len(), 1, "the message was deleted");
    assert!(v.read(&in_trash[0]).await.unwrap().is_some());
}

#[tokio::test]
async fn expunge_in_trash_deletes_for_good() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let trash = v.mailbox("Trash").await.unwrap().unwrap();
    let m = v.messages(&inbox).await.unwrap()[0].clone();
    v.move_to(&inbox, &m, &trash).await.unwrap();
    let doomed = v.messages(&trash).await.unwrap();
    let gone = v.expunge(&trash, &doomed).await;
    assert!(gone.failed.is_none());
    assert_eq!(gone.removed, [doomed[0].uid]);
    assert!(uids(&v, "Trash").await.is_empty());
    assert!(uids(&v, "INBOX").await.is_empty());
}

/// A label is a mailbox of its own: copying a message in puts the label on,
/// the message stays filed, and expunging there takes the label off
/// without deleting anything.
#[tokio::test]
async fn a_label_is_a_mailbox_that_copies_label_and_expunges_unlabel() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.deliver("Subject: two\r\n\r\n2");
    dir.add_label("7", "Work");
    let v = view(&dir).await;
    assert!(
        v.mailbox_names()
            .await
            .unwrap()
            .contains(&"Work".to_string())
    );

    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let work = v.mailbox("work").await.unwrap().unwrap();
    let empty = v.select(&work).await.unwrap().uidvalidity;
    let first = v.messages(&inbox).await.unwrap()[0].clone();
    let filed = v.copy(&inbox, &first, &work).await.unwrap();
    assert_eq!(uids(&v, "Work").await, [filed.uid]);
    assert_eq!(uids(&v, "INBOX").await, [1, 2], "a label moves nothing");
    let after = v.select(&work).await.unwrap();
    assert_ne!(after.uidvalidity, empty, "a client would not look again");
    assert_eq!(filed.uidvalidity, after.uidvalidity);

    let gone = v.expunge(&work, &after.messages).await;
    assert!(gone.failed.is_none());
    assert!(uids(&v, "Work").await.is_empty());
    assert_eq!(uids(&v, "INBOX").await, [1, 2], "the expunge deleted mail");
}

#[tokio::test]
async fn moving_out_of_a_label_takes_it_off_and_into_trash_trashes() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.add_label("7", "Work");
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let work = v.mailbox("Work").await.unwrap().unwrap();
    let trash = v.mailbox("Trash").await.unwrap().unwrap();
    let m = v.messages(&inbox).await.unwrap()[0].clone();
    v.copy(&inbox, &m, &work).await.unwrap();

    let shown = v.messages(&work).await.unwrap()[0].clone();
    v.move_to(&work, &shown, &inbox).await.unwrap();
    assert!(uids(&v, "Work").await.is_empty());
    assert_eq!(uids(&v, "INBOX").await, [1]);

    v.copy(&inbox, &m, &work).await.unwrap();
    let shown = v.messages(&work).await.unwrap()[0].clone();
    v.move_to(&work, &shown, &trash).await.unwrap();
    assert_eq!(uids(&v, "Trash").await.len(), 1);
    assert!(uids(&v, "INBOX").await.is_empty());
    assert!(
        uids(&v, "Work").await.is_empty(),
        "the Trash is out of every label"
    );
}
