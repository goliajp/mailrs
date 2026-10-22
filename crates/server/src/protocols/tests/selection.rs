//! Leaving a mailbox: UNSELECT keeps what is marked `\Deleted`, CLOSE
//! removes it without a word when the mailbox was opened read-write.

use super::Step;
use super::steps::{fetch, logout, opened, signed_in, step};

/// INBOX holds the two messages the first conversation left.
pub(super) fn unselect() -> Vec<Step> {
    let mut steps = signed_in("u1");
    steps.extend([
        opened("u2", "SELECT", 2, 4),
        step(
            "u3 STORE 2 +FLAGS.SILENT (\\Deleted)\r\n",
            &["u3 OK STORE completed\r\n"],
        ),
        step("u4 UNSELECT\r\n", &["u4 OK UNSELECT completed\r\n"]),
        step(
            "u5 FETCH 1 (FLAGS)\r\n",
            &["u5 NO not in SELECTED state\r\n"],
        ),
        step("u6 UNSELECT\r\n", &["u6 BAD not in SELECTED state\r\n"]),
        opened("u7", "SELECT", 2, 4),
        step(
            "u8 STORE 2 -FLAGS (\\Deleted)\r\n",
            &[
                &fetch(2, &[("FLAGS", "()".into())]),
                "u8 OK STORE completed\r\n",
            ],
        ),
        logout("u9"),
    ]);
    steps
}

/// INBOX holds the same two messages; CLOSE leaves it with the first.
pub(super) fn close() -> Vec<Step> {
    let mut steps = signed_in("c1");
    steps.extend([
        opened("c2", "SELECT", 2, 4),
        step(
            "c3 STORE 2 +FLAGS.SILENT (\\Deleted)\r\n",
            &["c3 OK STORE completed\r\n"],
        ),
        opened("c4", "EXAMINE", 2, 4),
        step("c5 CLOSE\r\n", &["c5 OK CLOSE completed\r\n"]),
        opened("c6", "SELECT", 2, 4),
        step("c7 CLOSE\r\n", &["c7 OK CLOSE completed\r\n"]),
        step("c8 CLOSE\r\n", &["c8 BAD not in SELECTED state\r\n"]),
        opened("c9", "SELECT", 1, 4),
        logout("c10"),
    ]);
    steps
}
