//! Reading a POP3 server's answers.
//!
//! POP3 is small enough that the parsing is not the interesting part.
//! The interesting part is **identity**: a client that cannot tell one
//! message from another downloads the mailbox again on every sync, and
//! the only durable identity POP3 offers is `UIDL` — which is
//! optional, and a server without it cannot be deduplicated at all.

use mailrs_pop3_client::{Line, Uid, parse_line, parse_uidl};

#[test]
fn a_positive_answer_is_told_from_a_negative_one() {
    assert!(matches!(parse_line("+OK 3 messages"), Line::Ok(_)));
    assert!(matches!(parse_line("-ERR no such message"), Line::Err(_)));
    // Neither, which is what every data line is.
    assert!(matches!(parse_line("1 abc123"), Line::Data(_)));
}

/// The failure that must not be retried on a timer: waiting cannot fix
/// a password that changed, and some providers count the attempts.
#[test]
fn a_refused_login_is_recognised() {
    for line in [
        "-ERR [AUTH] Authentication failed",
        "-ERR invalid password",
        "-ERR Login failed, please check your account",
    ] {
        assert!(
            mailrs_pop3_client::is_authentication_failure(line),
            "{line} was not read as a refused login"
        );
    }
    assert!(!mailrs_pop3_client::is_authentication_failure(
        "-ERR mailbox locked, try later"
    ));
}

/// `UIDL` is the whole of deduplication. Message *numbers* are
/// per-session and renumber the moment anything is deleted, so a
/// client that remembers those re-downloads the mailbox.
#[test]
fn uidl_lines_carry_the_number_and_the_identity() {
    let got = parse_uidl(&["1 whqtswO00WBw418f9t5JxYwZ", "2 QhdPYR:00WBw1Ph7x7"]);
    assert_eq!(
        got,
        vec![
            Uid {
                number: 1,
                uid: "whqtswO00WBw418f9t5JxYwZ".into()
            },
            Uid {
                number: 2,
                uid: "QhdPYR:00WBw1Ph7x7".into()
            },
        ]
    );
}

#[test]
fn a_malformed_uidl_line_is_skipped_rather_than_guessed_at() {
    let got = parse_uidl(&["1 good", "nonsense", "", "x y", "3"]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].uid, "good");
}

/// A uid may contain anything printable, including spaces in the wild
/// — the split is on the *first* space only.
#[test]
fn a_uid_containing_a_space_survives() {
    let got = parse_uidl(&["7 abc def"]);
    assert_eq!(got[0].uid, "abc def");
}

/// What to fetch, given what we already hold.
#[test]
fn only_what_is_new_is_fetched() {
    let held = ["a".to_string(), "b".to_string()];
    let on_server = parse_uidl(&["1 a", "2 b", "3 c"]);
    let want = mailrs_pop3_client::not_yet_held(&on_server, &held);
    assert_eq!(want.len(), 1);
    assert_eq!(want[0].number, 3);
}

/// A server with no UIDL cannot be deduplicated, and the honest answer
/// is to say so at set-up rather than to re-download the mailbox on
/// every sync for as long as the account exists.
#[test]
fn a_server_without_uidl_is_a_named_failure() {
    assert!(mailrs_pop3_client::no_uidl("-ERR unknown command"));
    assert!(!mailrs_pop3_client::no_uidl("+OK"));
}
