//! An account without a usable password signs in nowhere. One has an empty
//! hash, which is what clearing a password leaves; the other holds a value
//! an old import stored in clear, which is never compared as text, so
//! typing that value is refused too.

use base64::Engine;

use super::Step;
use super::harness::HOST;
use super::script::CAPS;
use super::steps::step;

/// Its stored hash is empty.
pub(super) const BLANK: &str = "blank@example.test";
/// Its stored value is this, in clear.
pub(super) const CLEAR: &str = "clear@example.test";
pub(super) const CLEAR_VALUE: &str = "clear-text-1";

fn plain(user: &str, password: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("\0{user}\0{password}"))
}

/// Who tries, with what: the empty password and the stored value itself.
const TRIES: [(&str, &str); 4] = [
    (BLANK, ""),
    (BLANK, "anything"),
    (CLEAR, CLEAR_VALUE),
    (CLEAR, ""),
];

/// SASL PLAIN without a password is refused as malformed before any check.
fn authenticate_refusal(password: &str) -> &'static str {
    match password.is_empty() {
        true => "a2 BAD malformed PLAIN response\r\n",
        false => "a2 NO invalid credentials\r\n",
    }
}

pub(super) fn imap() -> Vec<Vec<Step>> {
    let greeting = format!("* OK [CAPABILITY {CAPS}] {HOST} ready\r\n");
    TRIES
        .iter()
        .map(|(user, password)| {
            vec![
                step("", &[&greeting]),
                step(
                    &format!("a1 LOGIN {user} \"{password}\"\r\n"),
                    &["a1 NO invalid credentials\r\n"],
                ),
                step(
                    &format!("a2 AUTHENTICATE PLAIN {}\r\n", plain(user, password)),
                    &[authenticate_refusal(password)],
                ),
            ]
        })
        .collect()
}

pub(super) fn pop3() -> Vec<Vec<Step>> {
    // POP3 has no way to send an empty password: PASS needs an argument
    TRIES
        .iter()
        .filter(|(_, password)| !password.is_empty())
        .map(|(user, password)| {
            vec![
                step("", &["+OK mailrs POP3 ready\r\n"]),
                step(&format!("USER {user}\r\n"), &["+OK send PASS\r\n"]),
                step(
                    &format!("PASS {password}\r\n"),
                    &["-ERR invalid credentials\r\n"],
                ),
            ]
        })
        .collect()
}

pub(super) fn managesieve() -> Vec<Vec<Step>> {
    TRIES
        .iter()
        .map(|(user, password)| {
            vec![
                step(
                    "",
                    &[
                        "\"IMPLEMENTATION\" \"mailrs\"\r\n",
                        "\"SIEVE\" \"fileinto reject vacation envelope\"\r\n",
                        "\"SASL\" \"PLAIN\"\r\n",
                        "\"VERSION\" \"1.0\"\r\n",
                        "OK\r\n",
                    ],
                ),
                step(
                    &format!("AUTHENTICATE \"PLAIN\" \"{}\"\r\n", plain(user, password)),
                    &["NO \"Authentication failed\"\r\n"],
                ),
            ]
        })
        .collect()
}
