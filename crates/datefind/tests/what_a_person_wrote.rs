//! What this reads out of ordinary sentences, and — more importantly —
//! what it refuses to read.
//!
//! A wrong offer is worse than none: a client that proposes the wrong
//! meeting teaches the reader to ignore the button, and then the right
//! offer is ignored too.

use chrono::{NaiveDate, NaiveTime};
use mailrs_datefind::find;

fn on(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn at(h: u32, m: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h, m, 0)
}

fn reference() -> NaiveDate {
    on(2026, 8, 19)
}

#[test]
fn the_shapes_a_person_writes() {
    for (text, date, time) in [
        (
            "Let's meet on 2026-08-21 at 14:00.",
            on(2026, 8, 21),
            at(14, 0),
        ),
        ("Shall we say August 21 at 2pm?", on(2026, 8, 21), at(14, 0)),
        (
            "How about Aug 21st, 2026 at 2:30 PM?",
            on(2026, 8, 21),
            at(14, 30),
        ),
        ("21 August works for me", on(2026, 8, 21), None),
        ("8月21日 14:00 でお願いします", on(2026, 8, 21), at(14, 0)),
        ("2026年8月21日(金) 14時30分〜", on(2026, 8, 21), at(14, 30)),
        ("8月21日 午後2時から", on(2026, 8, 21), at(14, 0)),
    ] {
        let got = find(text, reference());
        assert_eq!(got.len(), 1, "one date in {text:?}, got {got:?}");
        assert_eq!(got[0].date, date, "in {text:?}");
        assert_eq!(got[0].time, time, "in {text:?}");
    }
}

/// The year a bare date means. Mail proposing a meeting proposes a
/// future one: "August 21" written in December is next August, not the
/// one eight months gone.
#[test]
fn a_bare_date_rolls_forward_rather_than_back() {
    let december = on(2026, 12, 20);
    let got = find("Can we do August 21?", december);
    assert_eq!(got[0].date, on(2027, 8, 21));

    // But a date that has only just passed is what the writer meant —
    // people write about last week.
    let got = find("the note from August 21", on(2026, 9, 1));
    assert_eq!(got[0].date, on(2026, 8, 21));
}

/// What it must not read. Each of these turns up in real mail, and each
/// would produce an offer that is simply wrong.
#[test]
fn what_it_refuses_to_guess() {
    for text in [
        // Ambiguous by construction: the ninth of August to half the
        // world, the eighth of September to the other half.
        "invoice 08/09 attached",
        // A number is a number.
        "see you 3",
        "section 21 of the agreement",
        // A serial number that happens to start like a date.
        "order 2026-08-2100045",
        // A month inside a word.
        "the marching band",
    ] {
        let got = find(text, reference());
        assert!(got.is_empty(), "read something out of {text:?}: {got:?}");
    }
}

/// A newsletter is not a meeting proposal, and twenty offers are no
/// offer at all.
#[test]
fn a_wall_of_dates_is_capped() {
    let text = (1..=20)
        .map(|d| format!("August {d} something happens. "))
        .collect::<String>();
    assert_eq!(find(&text, reference()).len(), mailrs_datefind::LIMIT);
}

/// The span names what was read, so a client can underline exactly that
/// rather than the whole sentence.
#[test]
fn the_span_names_what_it_read() {
    let text = "Let's meet on 2026-08-21 at 14:00.";
    let c = &find(text, reference())[0];
    assert_eq!(&text[c.span.0..c.span.1], "2026-08-21");
    assert_eq!(c.text, "2026-08-21");
}
