//! An expunged message takes its sender off the conversation's participant
//! list, on both cores: the list is the senders of the messages the reader
//! still holds.

use std::future::Future;
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::Client;

const FIRST: &str = "From: carol@outside.example\r\nTo: alice@example.test\r\n\
Subject: quote\r\nMessage-ID: <exp-1@outside.example>\r\n\
Date: Tue, 14 Nov 2023 10:00:00 +0000\r\n\r\nfirst\r\n";

const SECOND: &str = "From: dave@outside.example\r\nTo: alice@example.test\r\n\
Subject: Re: quote\r\nMessage-ID: <exp-2@outside.example>\r\n\
In-Reply-To: <exp-1@outside.example>\r\nReferences: <exp-1@outside.example>\r\n\
Date: Tue, 14 Nov 2023 11:00:00 +0000\r\n\r\nsecond\r\n";

const TID: &str = "exp-1@outside.example";

/// `participants(tid)` reads the list the core's conversation row shows.
pub(super) async fn expunged_sender<D, Q, QF>(dir: Arc<D>, participants: Q) -> Vec<String>
where
    D: Directory,
    Q: Fn(String) -> QF,
    QF: Future<Output = String>,
{
    let mut diffs = Vec::new();
    let mut c = Client::imap(dir).await;
    c.append("x0", "INBOX", "()", FIRST).await;
    c.append("x1", "INBOX", "()", SECOND).await;
    let before = participants(TID.into()).await;
    if before != "carol@outside.example,dave@outside.example" {
        diffs.push(format!(
            "expunge: both senders listed first\n    got: {before:?}"
        ));
    }
    // Deleting in INBOX moves the conversation to the Trash; expunging one
    // message there removes it for good.
    let inbox = expunge_quote(&mut c, "a", "INBOX").await;
    let trash = expunge_quote(&mut c, "b", "Trash").await;
    c.cmd("x6", "LOGOUT").await;
    let after = participants(TID.into()).await;
    if after != "dave@outside.example" {
        diffs.push(format!(
            "expunge: Carol's message is gone and so is she\n    got: {after:?}\n{inbox}{trash}"
        ));
    }
    diffs
}

/// Delete the message about "quote" in `folder` and expunge it; returns the
/// replies, for a difference to show.
async fn expunge_quote(c: &mut Client, tag: &str, folder: &str) -> String {
    let mut out = c.cmd(&format!("{tag}1"), &format!("SELECT {folder}")).await;
    let r = c
        .cmd(
            &format!("{tag}2"),
            "FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (SUBJECT)])",
        )
        .await;
    let mut uid = String::new();
    let mut current = String::new();
    for line in r.lines() {
        if let Some(rest) = line.split(" FETCH (UID ").nth(1) {
            current = rest.split(' ').next().unwrap_or("").to_string();
        }
        if line.trim() == "Subject: quote" {
            uid = current.clone();
        }
    }
    out.push_str(&r);
    let stored = c
        .cmd(
            &format!("{tag}3"),
            &format!("UID STORE {uid} +FLAGS (\\Deleted)"),
        )
        .await;
    out.push_str(&stored);
    let expunged = c
        .cmd(&format!("{tag}4"), &format!("UID EXPUNGE {uid}"))
        .await;
    out.push_str(&expunged);
    out
}
