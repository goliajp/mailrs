//! Addresses on the mock domain that answer with a fixed result, so a
//! sender can see how its code handles a bounce, an auto-reply, a delayed
//! delivery or a spam complaint without a real mailbox producing one.
//!
//! The replies are built here and go back to whoever sent the message.
//! None of them touches the deployment's own suppression list or feedback
//! loop processing.

/// What a simulation address answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Simulation {
    /// a permanent failure: a DSN with `Action: failed`
    Bounce,
    /// accepted, then an out-of-office auto-reply
    OutOfOffice,
    /// a DSN with `Action: delayed`; the message counts as delivered
    Delay,
    /// accepted, then an RFC 5965 abuse report about it
    Complaint,
}

impl Simulation {
    /// The simulation `addr` names on `domain`: `bounce@`, `ooto@`,
    /// `delay@` or `complaint@`, each also with a `+tag`.
    pub fn of(addr: &str, domain: &str) -> Option<Self> {
        if !crate::is_mock_domain(addr, domain) {
            return None;
        }
        let addr = addr.trim().trim_matches(['<', '>']);
        let (local, _) = addr.rsplit_once('@')?;
        let local = local.to_ascii_lowercase();
        let base = local.split('+').next().unwrap_or_default();
        match base {
            "bounce" => Some(Self::Bounce),
            "ooto" => Some(Self::OutOfOffice),
            "delay" => Some(Self::Delay),
            "complaint" => Some(Self::Complaint),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bounce => "bounce",
            Self::OutOfOffice => "ooto",
            Self::Delay => "delay",
            Self::Complaint => "complaint",
        }
    }

    /// Whether the original message counts as delivered.
    pub fn delivered(self) -> bool {
        !matches!(self, Self::Bounce)
    }

    /// The SMTP-style status a sender records for the recipient.
    pub fn status_line(self) -> &'static str {
        match self {
            Self::Bounce => "550 5.1.1 simulated bounce: mailbox does not exist",
            Self::OutOfOffice => "250 2.0.0 simulated delivery, out-of-office reply sent",
            Self::Delay => "250 2.0.0 simulated delivery after a delay notice",
            Self::Complaint => "250 2.0.0 simulated delivery, abuse report sent",
        }
    }
}

/// The message a simulation sends back to `sender` about `original`,
/// which was addressed to `rcpt`. `now` is unix seconds, passed in so the
/// output is reproducible.
pub fn reply(
    sim: Simulation,
    domain: &str,
    sender: &str,
    rcpt: &str,
    original: &[u8],
    now: i64,
) -> Vec<u8> {
    let line = |s: &str| s.replace(['\r', '\n'], " ");
    let (sender, rcpt) = (line(sender.trim().trim_matches(['<', '>'])), line(rcpt));
    let date = rfc5322_date(now);
    let mid = format!("<sim-{}-{now}-{:x}@{domain}>", sim.name(), seed(original));
    let refs = original_message_id(original)
        .map(|m| format!("In-Reply-To: {m}\r\nReferences: {m}\r\n"))
        .unwrap_or_default();
    match sim {
        Simulation::Bounce => dsn(domain, &sender, &rcpt, &date, &mid, &refs, original, "failed", "5.1.1"),
        Simulation::Delay => dsn(domain, &sender, &rcpt, &date, &mid, &refs, original, "delayed", "4.4.7"),
        Simulation::OutOfOffice => format!(
            "From: {rcpt}\r\nTo: <{sender}>\r\nSubject: Out of office: {subject}\r\nDate: {date}\r\n\
             Message-ID: {mid}\r\n{refs}Auto-Submitted: auto-replied\r\nMIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n\
             I am out of the office and will reply when I am back.\r\n\r\n\
             (This is a simulated automatic reply.)\r\n",
            subject = line(&original_subject(original)),
        )
        .into_bytes(),
        Simulation::Complaint => mailrs_arf::build(&mailrs_arf::ReportSpec {
            from: &format!("feedback-loop@{domain}"),
            to: &format!("<{sender}>"),
            feedback_type: "abuse",
            user_agent: "mailrs-mock/1",
            original_mail_from: &sender,
            original_rcpt_to: &rcpt,
            arrival_date: &date,
            message_id: &mid,
            original,
        }),
    }
}

#[allow(clippy::too_many_arguments)] // one call site per action, each line a header
fn dsn(
    domain: &str,
    sender: &str,
    rcpt: &str,
    date: &str,
    mid: &str,
    refs: &str,
    original: &[u8],
    action: &str,
    status: &str,
) -> Vec<u8> {
    let boundary = format!("=_mock_dsn_{:x}", seed(mid.as_bytes()));
    let human = match action {
        "failed" => "Your message could not be delivered and will not be retried.",
        _ => {
            "Your message has not been delivered yet. Delivery will be retried; no action is needed."
        }
    };
    let mut out = format!(
        "From: Mail Delivery System <MAILER-DAEMON@{domain}>\r\nTo: <{sender}>\r\n\
         Subject: {subject}\r\nDate: {date}\r\nMessage-ID: {mid}\r\n{refs}\
         Auto-Submitted: auto-replied\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/report; report-type=delivery-status; boundary=\"{boundary}\"\r\n\r\n\
         --{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
         {human}\r\n\r\n    {rcpt}\r\n\r\n(This is a simulated delivery report.)\r\n\r\n\
         --{boundary}\r\nContent-Type: message/delivery-status\r\n\r\n\
         Reporting-MTA: dns; {domain}\r\n\r\nFinal-Recipient: rfc822; {rcpt}\r\n\
         Action: {action}\r\nStatus: {status}\r\nDiagnostic-Code: smtp; {code} simulated\r\n\r\n\
         --{boundary}\r\nContent-Type: text/rfc822-headers\r\n\r\n",
        subject = match action {
            "failed" => "Undelivered Mail Returned to Sender",
            _ => "Delayed Mail (still being retried)",
        },
        code = match action {
            "failed" => "550",
            _ => "451",
        },
    )
    .into_bytes();
    out.extend_from_slice(header_block(original));
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

fn header_block(message: &[u8]) -> &[u8] {
    let end = message
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 2)
        .unwrap_or(message.len());
    &message[..end]
}

fn original_subject(original: &[u8]) -> String {
    let msg = mailrs_rfc5322::Message::new(original);
    msg.header("Subject")
        .map(|v| String::from_utf8_lossy(v).trim().to_string())
        .unwrap_or_default()
}

fn original_message_id(original: &[u8]) -> Option<String> {
    let msg = mailrs_rfc5322::Message::new(original);
    msg.header("Message-ID")
        .map(|v| String::from_utf8_lossy(v).trim().to_string())
        .filter(|m| m.starts_with('<') && m.ends_with('>') && !m.contains(['\r', '\n']))
}

fn seed(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

// civil-from-days (Howard Hinnant), UTC
fn rfc5322_date(now: i64) -> String {
    let days = now.div_euclid(86_400);
    let secs = now.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {day} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "mock.example.test";
    const ORIGINAL: &[u8] =
        b"From: app@shop.example\r\nSubject: Welcome\r\nMessage-ID: <w1@shop.example>\r\n\r\nhello";

    #[test]
    fn the_four_addresses_and_their_tags() {
        assert_eq!(
            Simulation::of("bounce@mock.example.test", D),
            Some(Simulation::Bounce)
        );
        assert_eq!(
            Simulation::of("<OOTO+x@MOCK.example.test>", D),
            Some(Simulation::OutOfOffice)
        );
        assert_eq!(
            Simulation::of("delay+1@mock.example.test", D),
            Some(Simulation::Delay)
        );
        assert_eq!(
            Simulation::of("complaint@mock.example.test", D),
            Some(Simulation::Complaint)
        );
        assert_eq!(Simulation::of("bounce@example.test", D), None);
        assert_eq!(Simulation::of("bouncer@mock.example.test", D), None);
        assert_eq!(Simulation::of("bounce.x@mock.example.test", D), None);
    }

    #[test]
    fn a_bounce_is_a_failed_dsn_that_threads_into_the_original() {
        let raw = reply(
            Simulation::Bounce,
            D,
            "<app@shop.example>",
            "bounce@mock.example.test",
            ORIGINAL,
            1_792_195_200,
        );
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("To: <app@shop.example>"));
        assert!(text.contains("Action: failed"));
        assert!(text.contains("Status: 5.1.1"));
        assert!(text.contains("In-Reply-To: <w1@shop.example>"));
        assert!(
            text.contains("Subject: Welcome"),
            "the original headers are quoted"
        );
        assert!(!text.contains("hello"), "the original body is not");
        assert!(text.contains("Date: Sat, 17 Oct 2026 00:00:00 +0000"));
    }

    #[test]
    fn delay_ooto_and_complaint() {
        let at = |s| {
            String::from_utf8(reply(
                s,
                D,
                "app@shop.example",
                "x@mock.example.test",
                ORIGINAL,
                0,
            ))
            .unwrap()
        };
        assert!(at(Simulation::Delay).contains("Action: delayed"));
        let ooto = at(Simulation::OutOfOffice);
        assert!(ooto.contains("Subject: Out of office: Welcome"));
        assert!(ooto.contains("Auto-Submitted: auto-replied"));
        let report = mailrs_arf::parse(at(Simulation::Complaint).as_bytes()).unwrap();
        assert_eq!(report.feedback_type, "abuse");
        assert_eq!(report.complainant(), Some("x@mock.example.test"));
        assert!(at(Simulation::Bounce).contains("Thu, 1 Jan 1970 00:00:00 +0000"));
    }

    #[test]
    fn a_sender_cannot_inject_a_header() {
        let raw = reply(
            Simulation::OutOfOffice,
            D,
            "a@b.example\r\nBcc: c@d.example",
            "ooto@mock.example.test",
            b"Subject: x\r\n\r\n",
            0,
        );
        assert!(!String::from_utf8(raw).unwrap().contains("\r\nBcc:"));
    }
}
