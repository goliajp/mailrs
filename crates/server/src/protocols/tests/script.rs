//! The conversations, and the answers both cores must give.

use base64::Engine;
use mailrs_imap_proto::{
    format_exists, format_flags, format_list, format_recent, special_use_flag,
};

use super::Step;
use super::harness::{HOST, PASSWORD, USER};
use super::steps::{fetch, step};

pub(super) const MAIL: [(&str, &str); 3] =
    [("one", "first"), ("two", "second"), ("three", "third")];

pub(super) fn message(subject: &str, body: &str) -> String {
    format!(
        "From: bob@example.test\r\nTo: {USER}\r\nSubject: {subject}\r\nMessage-ID: <{subject}@example.test>\r\n\r\n{body}\r\n"
    )
}

pub(super) fn size(i: usize) -> usize {
    message(MAIL[i].0, MAIL[i].1).len()
}

pub(super) const CAPS: &str = "IMAP4rev1 AUTH=PLAIN IDLE NAMESPACE ENABLE UNSELECT UIDPLUS MOVE SPECIAL-USE SORT QUOTA CONDSTORE QRESYNC";

fn plain() -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("\0{USER}\0{PASSWORD}"))
}

fn list() -> Vec<String> {
    let mut out: Vec<String> = ["INBOX", "Drafts", "Junk", "Sent", "Trash"]
        .iter()
        .map(|n| format_list(&format!("\\HasNoChildren{}", special_use_flag(n)), "/", n))
        .collect();
    out.push("a3 OK LIST completed\r\n".into());
    out
}

fn appends() -> Vec<Step> {
    let mut steps = Vec::new();
    for (i, (subject, body)) in MAIL.iter().enumerate() {
        let tag = format!("p{i}");
        let raw = message(subject, body);
        steps.push(step(
            &format!("{tag} APPEND INBOX {{{}}}\r\n", raw.len()),
            &["+ Ready for literal data\r\n"],
        ));
        steps.push(step(
            &format!("{raw}\r\n"),
            &[&format!("{tag} OK [APPENDUID * *] APPEND completed\r\n")],
        ));
    }
    steps
}

fn select() -> Step {
    let all = ["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"];
    let lines = [
        format_flags(&all),
        format_exists(3),
        format_recent(3),
        "* OK [UIDVALIDITY *] Version 1\r\n".into(),
        "* OK [UIDNEXT 4] Predicted next UID\r\n".into(),
        "* OK [HIGHESTMODSEQ *] Modseq\r\n".into(),
        "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n".into(),
        "a4 OK [READ-WRITE] SELECT completed\r\n".into(),
    ];
    step("a4 SELECT INBOX\r\n", &lines.each_ref().map(String::as_str))
}

fn reads() -> Vec<Step> {
    let listed: Vec<String> = (0..3)
        .map(|i| {
            let n = i as u32 + 1;
            let items = [
                ("UID", n.to_string()),
                ("FLAGS", "()".into()),
                ("RFC822.SIZE", size(i).to_string()),
            ];
            fetch(n, &items)
        })
        .collect();
    let mut listed: Vec<&str> = listed.iter().map(String::as_str).collect();
    listed.push("a5 OK FETCH completed\r\n");
    let header = "Subject: two\r\n\r\n";
    let peek = fetch(
        2,
        &[(
            "BODY[HEADER.FIELDS (SUBJECT)]",
            format!("{{{}}}\r\n{header}", header.len()),
        )],
    );
    vec![
        step("a5 FETCH 1:3 (UID FLAGS RFC822.SIZE)\r\n", &listed),
        step(
            "a6 FETCH 2 (BODY.PEEK[HEADER.FIELDS (SUBJECT)])\r\n",
            &[&peek, "a6 OK FETCH completed\r\n"],
        ),
    ]
}

fn changes() -> Vec<Step> {
    let seen = fetch(1, &[("FLAGS", "(\\Seen)".into())]);
    let unflagged = fetch(2, &[("FLAGS", "()".into())]);
    vec![
        step(
            "a7 STORE 1 +FLAGS (\\Seen)\r\n",
            &[&seen, "a7 OK STORE completed\r\n"],
        ),
        step(
            "a8 UID SEARCH UNSEEN\r\n",
            &["* SEARCH 2 3\r\n", "a8 OK SEARCH completed\r\n"],
        ),
        step(
            "a9 SEARCH SUBJECT two\r\n",
            &["* SEARCH 2\r\n", "a9 OK SEARCH completed\r\n"],
        ),
        step(
            "a10 STORE 3 +FLAGS.SILENT (\\Deleted)\r\n",
            &["a10 OK STORE completed\r\n"],
        ),
        step("a11 IDLE\r\n", &["+ idling\r\n"]),
        step("DONE\r\n", &["a11 OK IDLE terminated\r\n"]),
        step(
            "a12 EXPUNGE\r\n",
            &["* 3 EXPUNGE\r\n", "a12 OK EXPUNGE completed\r\n"],
        ),
        step(
            "a13 FETCH 1:* (FLAGS)\r\n",
            &[&seen, &unflagged, "a13 OK FETCH completed\r\n"],
        ),
        step(
            "a14 LOGOUT\r\n",
            &[
                "* BYE mailrs logging out\r\n",
                "a14 OK LOGOUT completed\r\n",
            ],
        ),
    ]
}

/// Sign in, file three messages, read, flag, search, idle, expunge.
pub(super) fn imap() -> Vec<Step> {
    let greeting = format!("* OK [CAPABILITY {CAPS}] {HOST} ready\r\n");
    let list = list();
    let mut steps = vec![
        step("", &[&greeting]),
        step(
            "a1 LOGIN alice@example.test wrong\r\n",
            &["a1 NO invalid credentials\r\n"],
        ),
        step(
            &format!("a2 LOGIN {USER} \"{PASSWORD}\"\r\n"),
            &["a2 OK LOGIN completed\r\n"],
        ),
        step(
            "a3 LIST \"\" \"*\"\r\n",
            &list.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
    ];
    steps.extend(appends());
    steps.push(select());
    steps.extend(reads());
    steps.extend(changes());
    steps
}

/// AUTHENTICATE PLAIN with the initial response, as most clients sign in.
pub(super) fn authenticate() -> Vec<Step> {
    let greeting = format!("* OK [CAPABILITY {CAPS}] {HOST} ready\r\n");
    vec![
        step("", &[&greeting]),
        step(
            &format!("b1 AUTHENTICATE PLAIN {}\r\n", plain()),
            &["b1 OK LOGIN completed\r\n"],
        ),
        step(
            "b2 SELECT INBOX\r\n",
            &[
                &format_flags(&["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"]),
                &format_exists(2),
                &format_recent(2),
                "* OK [UIDVALIDITY *] Version 1\r\n",
                "* OK [UIDNEXT 4] Predicted next UID\r\n",
                "* OK [HIGHESTMODSEQ *] Modseq\r\n",
                "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n",
                "b2 OK [READ-WRITE] SELECT completed\r\n",
            ],
        ),
    ]
}

/// The two messages the IMAP part left, over POP3.
pub(super) fn pop3() -> Vec<Step> {
    let (one, two) = (size(0), size(1));
    let body = message(MAIL[1].0, MAIL[1].1);
    let mut retr: Vec<String> = vec![format!("+OK {two} octets\r\n")];
    retr.extend(body.split_inclusive("\r\n").map(str::to_string));
    retr.push(".\r\n".into());
    vec![
        step("", &["+OK mailrs POP3 ready\r\n"]),
        step(&format!("USER {USER}\r\n"), &["+OK send PASS\r\n"]),
        step(&format!("PASS {PASSWORD}\r\n"), &["+OK 2 messages\r\n"]),
        step("STAT\r\n", &[&format!("+OK 2 {}\r\n", one + two)]),
        step(
            "LIST\r\n",
            &[
                "+OK maildrop follows\r\n",
                &format!("1 {one}\r\n"),
                &format!("2 {two}\r\n"),
                ".\r\n",
            ],
        ),
        step(
            "RETR 2\r\n",
            &retr.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        step("QUIT\r\n", &["+OK bye\r\n"]),
    ]
}

/// Store a script, list it, read it back, delete it.
pub(super) fn managesieve() -> Vec<Step> {
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
            &format!("AUTHENTICATE \"PLAIN\" \"{}\"\r\n", plain()),
            &["OK \"Authenticated\"\r\n"],
        ),
        step(
            "PUTSCRIPT \"mailrs\" {5+}\r\nkeep;\r\n",
            &["OK \"Script stored\"\r\n"],
        ),
        step(
            "LISTSCRIPTS\r\n",
            &["\"mailrs\" ACTIVE\r\n", "OK \"Listscripts completed\"\r\n"],
        ),
        step(
            "GETSCRIPT \"mailrs\"\r\n",
            &["{5}\r\n", "keep;\r\n", "OK\r\n"],
        ),
        step("DELETESCRIPT \"mailrs\"\r\n", &["OK \"Deleted\"\r\n"]),
        step("LISTSCRIPTS\r\n", &["OK \"Listscripts completed\"\r\n"]),
        step("LOGOUT\r\n", &["OK \"Bye\"\r\n"]),
    ]
}
