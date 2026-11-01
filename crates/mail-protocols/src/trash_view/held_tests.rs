use super::*;
use crate::backend::{Directory, Protocol};
use crate::test_support::{Memory, MemoryAccount, PASSWORD, USER, directory};
use crate::trash_view::TrashView;

async fn view(dir: &Memory) -> TrashView<HeldView<MemoryAccount>> {
    let a = dir.login(USER, PASSWORD, Protocol::Imap).await.unwrap();
    TrashView::new(HeldView::new(a.unwrap()))
}

async fn uids(v: &TrashView<HeldView<MemoryAccount>>, name: &str) -> Vec<u32> {
    let mb = v.mailbox(name).await.unwrap().unwrap();
    v.messages(&mb)
        .await
        .unwrap()
        .iter()
        .map(|m| m.uid)
        .collect()
}

/// A held conversation leaves INBOX for Review, and a client reads it there.
#[tokio::test]
async fn a_held_message_is_in_review_and_not_in_the_inbox() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.deliver("Subject: two\r\n\r\n2");
    dir.hold(1);
    let v = view(&dir).await;
    let names = v.mailbox_names().await.unwrap();
    assert!(names.contains(&"Review".to_string()), "{names:?}");
    assert!(names.contains(&"Trash".to_string()), "{names:?}");
    assert_eq!(uids(&v, "INBOX").await, [2]);
    assert_eq!(uids(&v, "review").await, [1]);
    let review = v.mailbox("Review").await.unwrap().unwrap();
    let held = v.messages(&review).await.unwrap()[0].clone();
    let body = v.read(&held).await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Subject: one"));
    assert_eq!(v.select(&review).await.unwrap().messages[0].seqno, 1);
}

/// Releasing is a decision for the review screen: a client can neither move
/// a held message out nor put one in, nor expunge it.
#[tokio::test]
async fn review_is_read_only_to_a_client() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.deliver("Subject: two\r\n\r\n2");
    dir.hold(1);
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    let review = v.mailbox("Review").await.unwrap().unwrap();
    let held = v.messages(&review).await.unwrap()[0].clone();
    assert!(v.move_to(&review, &held, &inbox).await.is_err());
    let kept = v.messages(&inbox).await.unwrap()[0].clone();
    assert!(v.copy(&inbox, &kept, &review).await.is_err());
    let out = v.expunge(&review, &[held]).await;
    assert!(out.removed.is_empty() && out.failed.is_some());
    assert_eq!(uids(&v, "Review").await, [1]);
    assert_eq!(uids(&v, "INBOX").await, [2]);
}

/// The INBOX reports a held message as gone, so a client that kept a copy
/// from before the hold drops it on its next sync.
#[tokio::test]
async fn a_held_message_is_reported_vanished_from_the_inbox() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\n1");
    dir.hold(1);
    let v = view(&dir).await;
    let inbox = v.mailbox("INBOX").await.unwrap().unwrap();
    assert_eq!(v.vanished_since(&inbox, 0).await.unwrap(), [1]);
}
