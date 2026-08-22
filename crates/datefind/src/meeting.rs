//! Whether a message is about arranging to meet.
//!
//! Split from `lib.rs` when it passed the 500-line limit: reading a
//! date and deciding a message is a booking are separate jobs, and the
//! vocabulary is the part that will keep growing as phrasings turn up.

/// The words that make a message about meeting somebody.
///
/// Deliberately about *arranging* rather than about calendars: "予定"
/// and "schedule" alone appear in delivery notices, so the list leans
/// on the vocabulary of asking — meet, call, free, いかが, ご都合.
///
/// Matched case-insensitively over the writer's own text. A list this
/// short will miss phrasings; missing one costs a chip that had to be
/// typed by hand, while guessing wrongly puts a calendar button on a
/// bank statement, and only one of those was ever reported.
pub const MEETING_WORDS: [&str; 34] = [
    // English — arranging
    "meet",
    "meeting",
    "call",
    "sync",
    "catch up",
    "chat",
    "appointment",
    "interview",
    "schedule a",
    "reschedule",
    "book a",
    "slot",
    "are you free",
    "if you are free",
    "available",
    "availability",
    "works for you",
    "how about",
    "shall we",
    "let us know a time",
    "calendar invite",
    "invite for",
    // 日本語
    "打ち合わせ",
    "ミーティング",
    "面談",
    "面接",
    "会議",
    "ご都合",
    "都合はいかが",
    "空いてい",
    "お時間",
    "アポ",
    // 中文
    "会议",
    "开会",
];

/// Whether the writer's own text is about arranging to meet.
pub(crate) fn mentions_meeting(text: &str) -> bool {
    let lower = text.to_lowercase();
    MEETING_WORDS.iter().any(|w| lower.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_counts_and_announcing_does_not() {
        assert!(mentions_meeting("Could we meet on Tuesday?"));
        assert!(mentions_meeting("打ち合わせの件ですが"));
        assert!(mentions_meeting("Are you free Thursday?"));
        assert!(!mentions_meeting("Your subscription renews soon."));
        assert!(!mentions_meeting("お届け予定日は9月2日です。"));
    }

    /// The list leans on arranging, not on calendars: a delivery notice
    /// says 予定 and a shipping mail says schedule, and neither is an
    /// invitation.
    #[test]
    fn calendar_words_alone_do_not_count() {
        assert!(!mentions_meeting("Delivery is scheduled for September 2."));
        assert!(!mentions_meeting("配送予定は9月2日です。"));
    }

    #[test]
    fn it_is_case_insensitive() {
        assert!(mentions_meeting("SHALL WE MEET?"));
    }
}
