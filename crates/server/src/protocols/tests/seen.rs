//! Reading a message's body marks it `\Seen`, as STORE would; peeking at
//! it, reading only its header, or reading it in an EXAMINEd mailbox do
//! not.

use super::Step;
use super::harness::USER;
use super::moves::append;
use super::steps::{fetch, logout, opened, signed_in, step};

fn literal(text: &str) -> String {
    format!("{{{}}}\r\n", text.len())
}

/// INBOX holds uid 1, `\Seen`; this adds uids 4 and 5 and leaves them
/// unseen.
pub(super) fn seen() -> Vec<Step> {
    let header = format!(
        "From: bob@example.test\r\nTo: {USER}\r\nSubject: six\r\nMessage-ID: <six@example.test>\r\n\r\n"
    );
    let mut header_lines = vec![format!("* 2 FETCH (RFC822.HEADER {}", literal(&header))];
    header_lines.extend(header.split_inclusive("\r\n").map(str::to_string));
    header_lines.push(")\r\n".into());
    header_lines.push("s7 OK FETCH completed\r\n".into());
    let unseen = fetch(2, &[("FLAGS", "()".into())]);

    let mut steps = signed_in("s1");
    steps.extend(append("s2", "six", "OK [APPENDUID * *] APPEND completed"));
    steps.extend(append("s3", "seven", "OK [APPENDUID * *] APPEND completed"));
    steps.extend([
        opened("s4", "SELECT", 3, 6),
        step(
            "s5 FETCH 2 (BODY.PEEK[TEXT])\r\n",
            &[
                &format!("* 2 FETCH (BODY[TEXT] {}", literal("six\r\n")),
                "six\r\n",
                ")\r\n",
                "s5 OK FETCH completed\r\n",
            ],
        ),
        step(
            "s6 FETCH 2 (FLAGS)\r\n",
            &[&unseen, "s6 OK FETCH completed\r\n"],
        ),
        step(
            "s7 FETCH 2 (RFC822.HEADER)\r\n",
            &header_lines.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        step(
            "s8 FETCH 2 (FLAGS)\r\n",
            &[&unseen, "s8 OK FETCH completed\r\n"],
        ),
        step(
            "s9 FETCH 2 (BODY[TEXT])\r\n",
            &[
                &format!("* 2 FETCH (BODY[TEXT] {}", literal("six\r\n")),
                "six\r\n",
                " FLAGS (\\Seen))\r\n",
                "s9 OK FETCH completed\r\n",
            ],
        ),
        step(
            "s10 FETCH 3 (FLAGS RFC822.TEXT)\r\n",
            &[
                &format!(
                    "* 3 FETCH (FLAGS (\\Seen) RFC822.TEXT {}",
                    literal("seven\r\n")
                ),
                "seven\r\n",
                ")\r\n",
                "s10 OK FETCH completed\r\n",
            ],
        ),
        step(
            "s11 SEARCH UNSEEN\r\n",
            &["* SEARCH\r\n", "s11 OK SEARCH completed\r\n"],
        ),
        step(
            "s12 STORE 2:3 -FLAGS.SILENT (\\Seen)\r\n",
            &["s12 OK STORE completed\r\n"],
        ),
        opened("s13", "EXAMINE", 3, 6),
        step(
            "s14 FETCH 2 (BODY[TEXT])\r\n",
            &[
                &format!("* 2 FETCH (BODY[TEXT] {}", literal("six\r\n")),
                "six\r\n",
                ")\r\n",
                "s14 OK FETCH completed\r\n",
            ],
        ),
        step(
            "s15 FETCH 2 (FLAGS)\r\n",
            &[&unseen, "s15 OK FETCH completed\r\n"],
        ),
        logout("s16"),
    ]);
    steps
}
