//! Reading a Message-ID out of an ENVELOPE.
//!
//! The tenth field, past nine that include address lists — and an
//! address list holds display names, which hold spaces, parentheses
//! and escaped quotes. A wrong Message-ID is worse than a missing one:
//! it makes one message permanently invisible, or merges it with
//! another.

use mailrs_imap_client::parse_envelope;

const ORDINARY: &str = r#"* 12 FETCH (UID 4390 ENVELOPE ("Tue, 5 Aug 2026 09:00:00 +0900" "Your request" (("Support" NIL "support" "example.com")) (("Support" NIL "support" "example.com")) NIL (("Hao Li" NIL "lihao" "golia.jp")) NIL NIL NIL "<ticket1@example.com>"))"#;

#[test]
fn the_message_id_is_the_tenth_field() {
    let e = parse_envelope(ORDINARY).expect("an envelope");
    assert_eq!(e.uid, 4390);
    assert_eq!(e.message_id, "<ticket1@example.com>");
}

/// The one a naive split gets wrong: a display name with a space in it
/// shifts every field after it, and the tenth becomes somebody's
/// surname.
#[test]
fn a_display_name_with_spaces_does_not_shift_the_fields() {
    let line = ORDINARY.replace(r#""Support""#, r#""Support Desk Team""#);
    assert_eq!(
        parse_envelope(&line).unwrap().message_id,
        "<ticket1@example.com>"
    );
}

/// And a display name with a parenthesis in it, which is legal and
/// which a depth counter that ignores quoting reads as nesting.
#[test]
fn a_parenthesis_inside_a_quoted_name_is_not_nesting() {
    let line = ORDINARY.replace(r#""Support""#, r#""Support (EU)""#);
    assert_eq!(
        parse_envelope(&line).unwrap().message_id,
        "<ticket1@example.com>"
    );
}

#[test]
fn an_escaped_quote_inside_a_name_survives() {
    let line = ORDINARY.replace(r#""Support""#, r#""The \"Help\" Desk""#);
    assert_eq!(
        parse_envelope(&line).unwrap().message_id,
        "<ticket1@example.com>"
    );
}

/// A message with no Message-ID has none. Inventing one here would
/// make it a different message on every sync.
#[test]
fn a_nil_message_id_is_absent_rather_than_guessed() {
    let line = ORDINARY.replace(r#""<ticket1@example.com>""#, "NIL");
    assert!(parse_envelope(&line).is_none());
}

/// A truncated line is not a shorter envelope. Guessing at what the
/// missing half said is how a wrong Message-ID gets stored.
#[test]
fn a_truncated_envelope_is_refused() {
    let line = &ORDINARY[..ORDINARY.len() - 20];
    assert!(parse_envelope(line).is_none());
}

#[test]
fn nonsense_is_refused_rather_than_panicking() {
    for junk in [
        "",
        "* 12 FETCH (UID 1)",
        "* 12 FETCH (UID 1 ENVELOPE)",
        "* 12 FETCH (UID 1 ENVELOPE (",
        "* 12 FETCH (ENVELOPE (\"a\" \"b\"))",
        "\u{1F600}",
    ] {
        let _ = parse_envelope(junk);
    }
}
