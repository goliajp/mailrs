//! The pieces every conversation is written with.

use mailrs_imap_proto::{format_exists, format_fetch, format_flags, format_recent};

use super::Step;
use super::harness::{HOST, PASSWORD, USER};
use super::script::CAPS;

pub(super) fn step(send: &str, expect: &[&str]) -> Step {
    Step {
        send: send.to_string(),
        expect: expect.concat(),
    }
}

pub(super) fn fetch(seq: u32, items: &[(&str, String)]) -> String {
    let items: Vec<(String, String)> = items
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    format_fetch(seq, &items)
}

/// The greeting is read and the account signed in, as `tag`.
pub(super) fn signed_in(tag: &str) -> Vec<Step> {
    vec![
        step("", &[&format!("* OK [CAPABILITY {CAPS}] {HOST} ready\r\n")]),
        step(
            &format!("{tag} LOGIN {USER} \"{PASSWORD}\"\r\n"),
            &[&format!("{tag} OK LOGIN completed\r\n")],
        ),
    ]
}

/// `SELECT` (or `EXAMINE`) of INBOX holding `exists` messages.
pub(super) fn opened(tag: &str, verb: &str, exists: u32, uidnext: u32) -> Step {
    let all = ["\\Seen", "\\Answered", "\\Flagged", "\\Deleted", "\\Draft"];
    let (permanent, done) = match verb {
        "EXAMINE" => (
            "* OK [PERMANENTFLAGS ()] Read-only\r\n".to_string(),
            format!("{tag} OK [READ-ONLY] EXAMINE completed\r\n"),
        ),
        _ => (
            "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)] Limited\r\n"
                .to_string(),
            format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
        ),
    };
    let lines = [
        format_flags(&all),
        format_exists(exists),
        format_recent(exists),
        "* OK [UIDVALIDITY *] Version 1\r\n".into(),
        format!("* OK [UIDNEXT {uidnext}] Predicted next UID\r\n"),
        "* OK [HIGHESTMODSEQ *] Modseq\r\n".into(),
        permanent,
        done,
    ];
    step(
        &format!("{tag} {verb} INBOX\r\n"),
        &lines.each_ref().map(String::as_str),
    )
}

pub(super) fn logout(tag: &str) -> Step {
    step(
        &format!("{tag} LOGOUT\r\n"),
        &[
            "* BYE mailrs logging out\r\n",
            &format!("{tag} OK LOGOUT completed\r\n"),
        ],
    )
}
