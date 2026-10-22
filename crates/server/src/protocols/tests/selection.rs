//! Leaving a mailbox: UNSELECT keeps what is marked `\Deleted`.

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
