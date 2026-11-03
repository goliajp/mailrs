//! Archive and All Mail as IMAP shows them, on both cores: a conversation
//! archived on the web leaves INBOX for `Archive`; moving a message into
//! `Archive` archives its conversation and moving it back to INBOX brings
//! it back; a MOVE out of INBOX into a label archives, a COPY does not;
//! deleting in `Archive` moves the conversation to the Trash; and nothing
//! moves out of `All Mail`.

use std::future::Future;
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::harness::LABEL;
use super::script::message;

/// The uid of the message with `subject` in a FETCH of
/// `(UID BODY.PEEK[HEADER.FIELDS (SUBJECT)])`.
fn uid_of(reply: &str, subject: &str) -> Option<String> {
    let mut current = None;
    for line in reply.lines() {
        if let Some(rest) = line.split(" FETCH (UID ").nth(1) {
            current = rest.split(' ').next().map(String::from);
        }
        if line.trim() == format!("Subject: {subject}") {
            return current;
        }
    }
    None
}

fn uidvalidity(reply: &str) -> Option<String> {
    code(reply, "[UIDVALIDITY ").map(|w| w[0].to_string())
}

const LISTED: &str = "1:* (UID BODY.PEEK[HEADER.FIELDS (SUBJECT)])";

/// `archive_on_web` archives a conversation the way the web does on that
/// core; `archived` says whether the core has a conversation archived.
pub(super) async fn archive<D, A, AF, Q, QF>(
    dir: Arc<D>,
    archive_on_web: A,
    archived: Q,
) -> Vec<String>
where
    D: Directory,
    A: FnOnce(String) -> AF,
    AF: Future<Output = ()>,
    Q: Fn(String) -> QF,
    QF: Future<Output = bool>,
{
    let mut diffs = Vec::new();
    let mut check = |ok: bool, what: &str, got: &str| {
        if !ok {
            diffs.push(format!("archive: {what}\n    got: {got:?}"));
        }
    };
    let mut c = Client::imap(dir).await;
    for subject in ["arch-web", "arch-imap", "arch-move", "arch-copy"] {
        c.append("r0", "INBOX", "()", &message(subject, "body"))
            .await;
    }
    let tid = |s: &str| format!("{s}@example.test");

    // archived on the web
    archive_on_web(tid("arch-web")).await;
    c.cmd("r1", "SELECT INBOX").await;
    let r = c.cmd("r2", &format!("FETCH {LISTED}")).await;
    check(
        uid_of(&r, "arch-web").is_none(),
        "an archived conversation leaves INBOX",
        &r,
    );
    check(uid_of(&r, "arch-imap").is_some(), "the others stay", &r);
    let r = c.cmd("r3", "LIST \"\" \"*\"").await;
    check(
        r.contains("\\Archive) \"/\" Archive\r\n") && r.contains("\\All) \"/\" \"All Mail\"\r\n"),
        "Archive and All Mail are listed with their special use",
        &r,
    );
    let before = uidvalidity(&c.cmd("r4", "SELECT Archive").await);
    let r = c.cmd("r5", &format!("FETCH {LISTED}")).await;
    check(uid_of(&r, "arch-web").is_some(), "Archive holds it", &r);
    c.cmd("r6", "SELECT \"All Mail\"").await;
    let r = c.cmd("r7", &format!("FETCH {LISTED}")).await;
    check(
        uid_of(&r, "arch-web").is_some() && uid_of(&r, "arch-imap").is_some(),
        "All Mail holds both sides of the archive",
        &r,
    );

    // into Archive and back over IMAP
    c.cmd("r8", "SELECT INBOX").await;
    let r = c.cmd("r9", &format!("FETCH {LISTED}")).await;
    let uid = uid_of(&r, "arch-imap").unwrap_or_default();
    let r = c.cmd("r10", &format!("UID MOVE {uid} Archive")).await;
    check(r.contains("r10 OK "), "MOVE into Archive completes", &r);
    check(
        archived(tid("arch-imap")).await,
        "and archives the conversation",
        &r,
    );
    let r = c.cmd("r11", "SELECT Archive").await;
    check(
        uidvalidity(&r) != before,
        "Archive's UIDVALIDITY changes as it changes",
        &r,
    );
    let r = c.cmd("r12", &format!("FETCH {LISTED}")).await;
    let shown = uid_of(&r, "arch-imap").unwrap_or_default();
    let r = c.cmd("r13", &format!("UID MOVE {shown} INBOX")).await;
    check(r.contains("r13 OK "), "MOVE back to INBOX completes", &r);
    check(
        !archived(tid("arch-imap")).await,
        "and brings the conversation back",
        &r,
    );
    c.cmd("r14", "SELECT INBOX").await;
    let r = c.cmd("r15", &format!("FETCH {LISTED}")).await;
    check(
        uid_of(&r, "arch-imap").is_some(),
        "INBOX shows it again",
        &r,
    );

    // into a label: MOVE archives, COPY does not
    let moved = uid_of(&r, "arch-move").unwrap_or_default();
    let copied = uid_of(&r, "arch-copy").unwrap_or_default();
    let r = c.cmd("r16", &format!("UID MOVE {moved} {LABEL}")).await;
    check(r.contains("r16 OK "), "MOVE into the label completes", &r);
    check(
        archived(tid("arch-move")).await,
        "and archives, as Gmail's does",
        &r,
    );
    let r = c.cmd("r17", &format!("UID COPY {copied} {LABEL}")).await;
    check(r.contains("r17 OK "), "COPY into the label completes", &r);
    check(
        !archived(tid("arch-copy")).await,
        "and leaves the Inbox alone",
        &r,
    );
    c.cmd("r18", &format!("SELECT {LABEL}")).await;
    let r = c.cmd("r19", &format!("FETCH {LISTED}")).await;
    check(
        uid_of(&r, "arch-move").is_some() && uid_of(&r, "arch-copy").is_some(),
        "the label holds both",
        &r,
    );

    // deleting in Archive is deleting
    c.cmd("r20", "SELECT Archive").await;
    let r = c.cmd("r21", &format!("FETCH {LISTED}")).await;
    let doomed = uid_of(&r, "arch-web").unwrap_or_default();
    c.cmd(
        "r22",
        &format!("UID STORE {doomed} +FLAGS.SILENT (\\Deleted)"),
    )
    .await;
    let r = c.cmd("r23", &format!("UID EXPUNGE {doomed}")).await;
    check(r.contains("r23 OK "), "EXPUNGE in Archive completes", &r);
    c.cmd("r24", "SELECT Trash").await;
    let r = c.cmd("r25", &format!("FETCH {LISTED}")).await;
    check(
        uid_of(&r, "arch-web").is_some(),
        "and moves the conversation to the Trash",
        &r,
    );

    // All Mail is read-only
    c.cmd("r26", "SELECT \"All Mail\"").await;
    let r = c.cmd("r27", "MOVE 1 INBOX").await;
    check(
        r.contains("r27 NO [CANNOT]"),
        "nothing moves out of All Mail",
        &r,
    );
    diffs
}
