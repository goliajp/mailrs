use super::*;
use crate::backend::{Directory, Protocol};
use crate::test_support::{Memory, MemoryAccount, PASSWORD, USER, directory};

async fn view(dir: &Memory) -> TrashView<MemoryAccount> {
    let a = dir.login(USER, PASSWORD, Protocol::Imap).await.unwrap();
    TrashView::new(a.unwrap())
}

async fn uids(v: &TrashView<MemoryAccount>, name: &str) -> Vec<u32> {
    let mb = v.mailbox(name).await.unwrap().unwrap();
    let mut out: Vec<u32> = v
        .messages(&mb)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uid)
        .collect();
    out.sort_unstable();
    out
}

fn two() -> std::sync::Arc<Memory> {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.deliver("Subject: two\r\n\r\n2");
    dir
}

#[tokio::test]
async fn an_archived_conversation_leaves_inbox_for_archive() {
    let dir = two();
    dir.archive(1);
    let v = view(&dir).await;
    let names = v.mailbox_names().await.unwrap();
    assert!(names.contains(&"Archive".to_string()), "{names:?}");
    assert!(names.contains(&"All Mail".to_string()), "{names:?}");
    assert_eq!(uids(&v, "INBOX").await, [2]);
    assert_eq!(uids(&v, "Archive").await, [1]);
    assert_eq!(uids(&v, "all mail").await, [1, 2]);
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    assert_eq!(
        v.vanished_since(&inbox, 0).await.unwrap(),
        [1],
        "a client that synced before learns it left"
    );
}

#[tokio::test]
async fn moving_into_archive_and_back_archives_and_restores() {
    let dir = two();
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let archive = v.mailbox("Archive").await.unwrap().unwrap();
    let before = v.select(&archive).await.unwrap().uidvalidity;
    let first = v.messages(&inbox).await.unwrap()[0].clone();
    let filed = v.move_to(&inbox, &first, &archive).await.unwrap();
    assert_eq!(uids(&v, "INBOX").await, [2]);
    let after = v.select(&archive).await.unwrap();
    assert_eq!(after.messages.len(), 1);
    assert_eq!(filed.uid, after.messages[0].uid);
    assert_eq!(filed.uidvalidity, after.uidvalidity);
    assert_ne!(after.uidvalidity, before, "a client would not look again");

    let back = after.messages[0].clone();
    v.move_to(&archive, &back, &inbox).await.unwrap();
    assert_eq!(uids(&v, "INBOX").await, [1, 2]);
    assert!(uids(&v, "Archive").await.is_empty());
}

#[tokio::test]
async fn a_move_into_a_label_archives_and_a_copy_does_not() {
    let dir = two();
    dir.add_label("7", "Work");
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let work = v.mailbox("Work").await.unwrap().unwrap();
    let listed = v.messages(&inbox).await.unwrap();
    v.copy(&inbox, &listed[0], &work).await.unwrap();
    v.move_to(&inbox, &listed[1], &work).await.unwrap();
    assert_eq!(uids(&v, "Work").await, [1, 2]);
    assert_eq!(uids(&v, "INBOX").await, [1], "the moved one left INBOX");
    assert_eq!(uids(&v, "Archive").await, [2]);
}

#[tokio::test]
async fn deleting_in_archive_moves_the_conversation_to_the_trash() {
    let dir = two();
    dir.archive(1);
    let v = view(&dir).await;
    let archive = v.mailbox("Archive").await.unwrap().unwrap();
    let listed = v.messages(&archive).await.unwrap();
    let gone = v.expunge(&archive, &listed).await;
    assert!(gone.failed.is_none());
    assert_eq!(gone.removed, [1]);
    assert_eq!(uids(&v, "Trash").await, [1]);
    assert!(uids(&v, "Archive").await.is_empty());
    assert_eq!(uids(&v, "all mail").await, [2]);
}

#[tokio::test]
async fn all_mail_is_read_only_but_copies_out() {
    let dir = two();
    let v = view(&dir).await;
    let all = v.mailbox("All Mail").await.unwrap().unwrap();
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let archive = v.mailbox("Archive").await.unwrap().unwrap();
    let listed = v.messages(&all).await.unwrap();
    assert!(v.move_to(&all, &listed[0], &inbox).await.is_err());
    assert!(v.expunge(&all, &listed).await.failed.is_some());
    assert!(
        v.append(&all, b"Subject: x\r\n\r\nx".to_vec(), Vec::new())
            .await
            .is_err()
    );
    let inbox_listed = v.messages(&inbox).await.unwrap();
    assert!(v.copy(&inbox, &inbox_listed[0], &all).await.is_err());
    v.copy(&all, &listed[0], &archive).await.unwrap();
    assert_eq!(uids(&v, "Archive").await, [1]);
    assert_eq!(uids(&v, "INBOX").await, [2]);
}
