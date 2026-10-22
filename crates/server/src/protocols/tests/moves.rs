//! MOVE tells the client the moved messages left, as EXPUNGE would.

use super::Step;
use super::script::message;
use super::steps::{fetch, logout, signed_in, step};

/// `tag APPEND INBOX` of a new message about `subject`.
pub(super) fn append(tag: &str, subject: &str, answer: &str) -> Vec<Step> {
    let raw = message(subject, subject);
    vec![
        step(
            &format!("{tag} APPEND INBOX {{{}}}\r\n", raw.len()),
            &["+ Ready for literal data\r\n"],
        ),
        step(&format!("{raw}\r\n"), &[&format!("{tag} {answer}\r\n")]),
    ]
}

/// INBOX holds uids 1, 4 and 5; this leaves uid 1 there and moves 4 to
/// Trash and 5 to Junk.
///
/// The SQL store gives a copy a new uid and the Maildir one keeps the
/// message's own, so after this the two stores' uids differ.
pub(super) fn moves() -> Vec<Step> {
    let mut steps = signed_in("m1");
    steps.extend([
        step(
            "m4 SELECT INBOX\r\n",
            &[
                "* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)\r\n",
                "* 3 EXISTS\r\n",
                "* 3 RECENT\r\n",
                "* OK [UIDVALIDITY *] Version 1\r\n",
                "* OK [UIDNEXT 6] Predicted next UID\r\n",
                "* OK [HIGHESTMODSEQ *] Modseq\r\n",
                "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n",
                "m4 OK [READ-WRITE] SELECT completed\r\n",
            ],
        ),
        step(
            "m5 MOVE 2 Trash\r\n",
            &["* 2 EXPUNGE\r\n", "m5 OK MOVE completed\r\n"],
        ),
        step(
            "m6 FETCH 1:* (UID)\r\n",
            &[
                &fetch(1, &[("UID", "1".into())]),
                &fetch(2, &[("UID", "5".into())]),
                "m6 OK FETCH completed\r\n",
            ],
        ),
        step(
            "m7 ENABLE QRESYNC\r\n",
            &["* ENABLED QRESYNC\r\n", "m7 OK ENABLE completed\r\n"],
        ),
        step(
            "m8 UID MOVE 5 Junk\r\n",
            &["* VANISHED 5\r\n", "m8 OK MOVE completed\r\n"],
        ),
        step(
            "m9 FETCH 1:* (UID)\r\n",
            &[
                &fetch(1, &[("UID", "1".into())]),
                "m9 OK FETCH completed\r\n",
            ],
        ),
        logout("m10"),
    ]);
    steps
}
