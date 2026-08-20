//! The answer an attendee sends back.
//!
//! RFC 5546 §3.2.3: a `REPLY` echoes the request's `UID` and `SEQUENCE`,
//! carries the organiser it is addressed to, and **exactly one
//! `ATTENDEE` — the person replying**. Sending back the whole guest list
//! is the classic mistake: Exchange reads it as one attendee answering
//! on behalf of everybody and either rejects the reply or, worse,
//! rewrites the other guests' states.
//!
//! Built here rather than in a handler because the shape is the
//! protocol's, not one server's, and because a reply that is subtly
//! wrong fails in the organiser's client, where nobody on this side
//! will ever see it.

use chrono::Utc;

use crate::{Attendee, IcalError, ParsedInvite, PartStat, Person, Role};

/// Build the `REPLY` for one attendee's answer to `request`.
///
/// `me` is the address replying — matched case-insensitively against the
/// request's attendee list, because the organiser's spelling of it
/// (`Hao.Li@…`) is what must go back out, not ours.
///
/// Returns `None` when `me` is not on the guest list: replying as
/// somebody the organiser never invited is not a reply, and a client
/// that sends one gets a bounce it cannot interpret.
pub fn build(request: &ParsedInvite, me: &str, answer: PartStat) -> Option<ParsedInvite> {
    let invited = request
        .attendees
        .iter()
        .find(|a| a.email.eq_ignore_ascii_case(me))?;

    Some(ParsedInvite {
        method: crate::Method::Reply,
        uid: request.uid.clone(),
        // Echoed, not incremented: the sequence identifies *which*
        // revision is being answered. An organiser who has since sent a
        // newer one can tell that this reply is about the older.
        sequence: request.sequence,
        dtstamp: Utc::now(),
        dtstart: request.dtstart.clone(),
        dtend: request.dtend.clone(),
        duration: request.duration,
        organizer: request.organizer.clone(),
        // Exactly one, and it keeps the organiser's spelling of the
        // address and the name they used.
        attendees: vec![Attendee {
            email: invited.email.clone(),
            cn: invited.cn.clone(),
            partstat: answer,
            role: invited.role,
            // An answer needs no answer.
            rsvp: false,
        }],
        rrule: request.rrule.clone(),
        exdate: Vec::new(),
        rdate: Vec::new(),
        // Present only when the request targeted one occurrence — the
        // reply must be about the same occurrence, and dropping it
        // answers for the whole series.
        recurrence_id: request.recurrence_id.clone(),
        status: None,
        summary: request.summary.clone(),
        location: request.location.clone(),
        description: None,
        // The zones the times refer to travel with them, or the
        // organiser's client resolves them against nothing.
        vtimezones: request.vtimezones.clone(),
    })
}

/// The `REPLY` as a `text/calendar` body.
pub fn serialize_reply(
    request: &ParsedInvite,
    me: &str,
    answer: PartStat,
) -> Result<String, IcalError> {
    let reply = build(request, me, answer).ok_or_else(|| {
        IcalError::InvalidSemantics(format!("{me} is not on this invitation's attendee list"))
    })?;
    crate::serialize::serialize(&reply)
}

/// The `PARTSTAT` a wire word means, or `None` for one an attendee
/// cannot send.
///
/// `NEEDS-ACTION` is deliberately absent: it is the state before an
/// answer, not an answer.
pub fn partstat_from_wire(s: &str) -> Option<PartStat> {
    match s.trim().to_ascii_uppercase().as_str() {
        "ACCEPTED" => Some(PartStat::Accepted),
        "DECLINED" => Some(PartStat::Declined),
        "TENTATIVE" => Some(PartStat::Tentative),
        _ => None,
    }
}

/// A subject line an organiser's client will recognise, in the form
/// every major implementation uses.
pub fn subject_for(answer: PartStat, summary: &str) -> String {
    let word = match answer {
        PartStat::Accepted => "Accepted",
        PartStat::Declined => "Declined",
        PartStat::Tentative => "Tentative",
        _ => "Reply",
    };
    format!("{word}: {summary}")
}

/// Who to address the reply to.
pub fn organizer_of(request: &ParsedInvite) -> Option<&Person> {
    request.organizer.as_ref()
}

/// The role an attendee had, for a reply that keeps it.
pub fn role_of(a: &Attendee) -> Role {
    a.role
}

/// The whole reply as an RFC 5322 message, ready to enqueue.
///
/// `multipart/alternative` with a human sentence and the
/// `text/calendar; method=REPLY` part beside it — the shape RFC 6047 §2.4
/// asks for, and the one every organiser's client already handles. A
/// bare calendar part with no text arrives in some clients as an empty
/// message.
///
/// The `Date` and `Message-ID` are the caller's: this crate has no clock
/// and no hostname, and inventing either here would make the output
/// untestable.
pub fn reply_message(
    request: &ParsedInvite,
    me: &str,
    answer: PartStat,
    date_rfc2822: &str,
    message_id: &str,
) -> Result<Vec<u8>, IcalError> {
    let organizer = organizer_of(request)
        .map(|o| o.email.clone())
        .ok_or_else(|| IcalError::InvalidSemantics("invitation names no organizer".into()))?;
    let ics = serialize_reply(request, me, answer)?;
    let word = match answer {
        PartStat::Accepted => "accepted",
        PartStat::Declined => "declined",
        PartStat::Tentative => "tentatively accepted",
        _ => "replied to",
    };
    let sentence = format!("{me} has {word} this invitation.\r\n");
    let boundary = format!("mailrs-reply-{}", message_id.trim_matches(['<', '>']));
    let mut out = String::new();
    out.push_str(&format!("From: {me}\r\n"));
    out.push_str(&format!("To: {organizer}\r\n"));
    out.push_str(&format!(
        "Subject: {}\r\n",
        subject_for(answer, &request.summary)
    ));
    out.push_str(&format!("Date: {date_rfc2822}\r\n"));
    out.push_str(&format!("Message-ID: {message_id}\r\n"));
    out.push_str("MIME-Version: 1.0\r\n");
    out.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n"
    ));
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str("Content-Type: text/plain; charset=utf-8\r\n\r\n");
    out.push_str(&sentence);
    out.push_str(&format!("\r\n--{boundary}\r\n"));
    // The method belongs on the part's Content-Type as well as inside
    // the body: some clients route on the header alone.
    out.push_str("Content-Type: text/calendar; charset=utf-8; method=REPLY\r\n\r\n");
    out.push_str(&ics);
    out.push_str(&format!("\r\n--{boundary}--\r\n"));
    Ok(out.into_bytes())
}
