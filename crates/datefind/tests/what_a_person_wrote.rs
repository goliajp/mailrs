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

// ── what a proposal is, and what merely contains dates ──────────────
//
// 2026-08-21, from a screenshot: a support reply carried eight chips —
// `Aug 21 2026`, `2026-08-20`, `2026-08-21` three times, `2026-08-19`
// twice, `2026-08-17`. Every one of them came out of the quoted block
// below the reply, where an earlier message had pasted SMTP rejection
// timestamps. Nobody was proposing anything.
//
// `find` was right each time: those are dates. The defect is that
// finding a date and reading a proposal are different questions, and
// only the first one was being asked.

/// The message from the screenshot, reduced to its shape.
const SUPPORT_REPLY: &str = "\
Hello,

I have received your request for an investigation into your IP address
[52.195.89.111]. Please re-submit the IP through the automation system.

Thanks again,
Tejesh S

On Aug 21 2026, at 16:52, Hao Li <lihao@golia.jp> wrote:
> The block is still in place as of a few minutes ago:
>     2026-08-21T16:52:06.340Z 08DEFEB847463EDB
>     Earlier, identical:
>     [MxId=11BDF1D5C5014D10]  2026-08-19T12:33:12Z
>     [MxId=11BDF1D8AA3F31F1]  2026-08-19T03:58:46Z
>     [MxId=11BDF1F3D093ED1F]  2026-08-17T05:06:08Z   (first failure)
> The automated review answered on 2026-08-20 and again on 2026-08-21.
";

#[test]
fn a_support_reply_full_of_timestamps_proposes_nothing() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    let found = mailrs_datefind::propose(SUPPORT_REPLY, reference);
    assert!(
        found.is_empty(),
        "quoted rejection timestamps were offered as events: {:?}",
        found.iter().map(|c| c.text.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a_person_proposing_two_times_still_gets_both() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    let text = "Could we meet on August 25 at 2pm? \
                If that is bad for you, August 26 at 10am also works.";
    let found = mailrs_datefind::propose(text, reference);
    assert_eq!(
        found.len(),
        2,
        "a two-option proposal: {:?}",
        found.iter().map(|c| c.text.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn the_same_day_written_twice_is_offered_once() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    // Both halves carry the hour now: a bare day is no longer offered
    // at all, so a dedupe test written without one would pass for the
    // wrong reason.
    let text = "Shall we say 2026-08-25 at 14:00? I am free on 2026-08-25 at 14:00.";
    assert_eq!(mailrs_datefind::propose(text, reference).len(), 1);
}

#[test]
fn a_date_already_past_is_not_a_proposal() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    let text = "As agreed on August 17, the migration is done.";
    assert!(mailrs_datefind::propose(text, reference).is_empty());
}

#[test]
fn a_machine_timestamp_is_not_a_proposal_even_unquoted() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    let text = "The next run is stamped 2026-08-25T04:00:00.123Z in the log.";
    assert!(
        mailrs_datefind::propose(text, reference).is_empty(),
        "an ISO instant with sub-seconds and a zone is a log line"
    );
}

#[test]
fn a_newsletter_of_dates_offers_none_rather_than_eight() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    let text = "Upcoming: 2026-08-25, 2026-08-26, 2026-08-27, \
                2026-08-28, 2026-08-29, 2026-08-30.";
    assert!(
        mailrs_datefind::propose(text, reference).is_empty(),
        "six dates is a listing, not a proposal"
    );
}

#[test]
fn find_itself_still_reads_every_date_it_is_given() {
    // `propose` is the judgement; `find` stays the plain reader, so the
    // two tests above cannot pass by breaking detection.
    let reference = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    assert!(mailrs_datefind::find(SUPPORT_REPLY, reference).len() >= 5);
}

/// A bank notice naming today is telling you what happened, not asking.
///
/// 2026-08-22, from a screenshot: a deposit notification carried an
/// "Add to calendar" chip for `8月22日`, its own arrival day. The
/// sentence is past tense — 「8月22日に…振込入金がありました」 — and the
/// date has no hour beside it, which is what a statement about today
/// looks like. A proposal for today names an hour, because otherwise
/// there is nothing to agree to.
#[test]
fn a_bare_date_that_is_todays_date_states_rather_than_asks() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 22).unwrap();
    let text = "2026年8月22日に、お客さまの代表口座円普通預金への振込入金がありました。";
    assert!(
        mailrs_datefind::propose(text, reference).is_empty(),
        "a deposit notice was offered as an event"
    );
}

#[test]
fn today_with_an_hour_is_still_a_proposal() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 22).unwrap();
    let text = "Are you free today, August 22 at 3pm?";
    assert_eq!(
        mailrs_datefind::propose(text, reference).len(),
        1,
        "somebody asking about this afternoon got nothing"
    );
}

// ── it must read as a meeting, not merely as a date ─────────────────
//
// 2026-08-23: "不要这么敏感，我只要能做好那种标准的 mtg 预约就行了".
// Every gate until now excluded a bad shape — quoted, machine, past,
// too many — so anything left through was offered, and a future date
// in prose is very often a deadline, a renewal or a delivery window.
//
// The gate is now positive: an hour, and a message that says somewhere
// that it is about meeting. A standard booking has both.

#[test]
fn a_deadline_with_an_hour_is_not_a_meeting() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "The report is due September 1 at 5pm — please send it before then.";
    assert!(mailrs_datefind::propose(text, reference).is_empty());
}

#[test]
fn a_renewal_notice_is_not_a_meeting() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "Your subscription renews on 2026-09-10 at 09:00 JST.";
    assert!(mailrs_datefind::propose(text, reference).is_empty());
}

#[test]
fn a_delivery_window_is_not_a_meeting() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "お届け予定日は9月2日 14:00〜16:00です。";
    assert!(mailrs_datefind::propose(text, reference).is_empty());
}

#[test]
fn a_meeting_without_an_hour_is_not_offered() {
    // Deliberate. An all-day chip for a meeting is not what anybody
    // wants, and a bare day is the shape every false positive took.
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "Let's meet on August 25 — I'll send an invite later.";
    assert!(mailrs_datefind::propose(text, reference).is_empty());
}

#[test]
fn the_standard_booking_still_works_in_english() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "Could we meet on August 25 at 2pm? If that is bad for you, \
                August 26 at 10am also works for me.";
    let got = mailrs_datefind::propose(text, reference);
    assert_eq!(
        got.len(),
        2,
        "{:?}",
        got.iter().map(|c| &c.text).collect::<Vec<_>>()
    );
}

#[test]
fn the_standard_booking_still_works_in_japanese() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "打ち合わせの件、9月2日 14:00 はいかがでしょうか。";
    assert_eq!(mailrs_datefind::propose(text, reference).len(), 1);
}

#[test]
fn an_interview_slot_is_a_meeting() {
    let reference = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let text = "We would like to schedule the interview for September 3 at 11:00.";
    assert_eq!(mailrs_datefind::propose(text, reference).len(), 1);
}
