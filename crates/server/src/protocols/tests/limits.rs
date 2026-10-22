//! A command line or a literal too large is refused before it is held,
//! before sign-in as after it.

use super::Step;
use super::harness::{HOST, PASSWORD, USER};
use super::script::CAPS;
use super::steps::{signed_in, step};

const CLOSED: &str = "<nothing more>\n";

fn long_line(tag: &str, len: usize) -> String {
    format!("{tag} NOOP {}\r\n", "a".repeat(len))
}

/// Each on its own connection, which the last step of each closes.
pub(super) fn imap() -> Vec<Vec<Step>> {
    let too_long = "* BAD [TOOBIG] line too long, closing\r\n";
    let before = vec![
        step("", &[&format!("* OK [CAPABILITY {CAPS}] {HOST} ready\r\n")]),
        step(&long_line("t1", 70 * 1024), &[too_long]),
        step("", &[CLOSED]),
    ];
    let mut after = signed_in("t1");
    after.extend([
        step(
            "t2 APPEND INBOX {60000000}\r\n",
            &["t2 NO [TOOBIG] messages are limited to 52428800 bytes\r\n"],
        ),
        step(&long_line("t3", 60 * 1024), &["t3 OK NOOP completed\r\n"]),
        step(&long_line("t4", 70 * 1024), &[too_long]),
        step("", &[CLOSED]),
    ]);
    vec![before, after]
}

pub(super) fn pop3() -> Vec<Vec<Step>> {
    let too_long = "-ERR line too long, closing\r\n";
    let user = format!("USER {}\r\n", "a".repeat(9000));
    let before = vec![
        step("", &["+OK mailrs POP3 ready\r\n"]),
        step(&user, &[too_long]),
        step("", &[CLOSED]),
    ];
    let after = vec![
        step("", &["+OK mailrs POP3 ready\r\n"]),
        step(&format!("USER {USER}\r\n"), &["+OK send PASS\r\n"]),
        step(&format!("PASS {PASSWORD}\r\n"), &["+OK 1 messages\r\n"]),
        step(&format!("NOOP {}\r\n", "a".repeat(8000)), &["+OK\r\n"]),
        step(&format!("NOOP {}\r\n", "a".repeat(9000)), &[too_long]),
        step("", &[CLOSED]),
    ];
    vec![before, after]
}
