import Testing

@testable import Mailrs

/// Reading a `Date:` header.
///
/// Getting this wrong is quiet: the row shows a plausible time that is
/// hours out, or the list orders itself by nothing at all.
@Suite struct MailDateTests {
    @Test func anOrdinaryDateIsRead() {
        // 2026-08-05 09:00:00 +0900 == 2026-08-05T00:00:00Z
        #expect(MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900") == 1_785_888_000)
    }

    /// The day name is optional, and servers disagree about the spaces.
    @Test func theDayNameIsOptional() {
        let withName = MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900")
        #expect(MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900") == withName)
        #expect(MailDate.epochSeconds("Tue,  5  Aug  2026  09:00:00  +0900") == withName)
    }

    @Test func secondsAreOptional() {
        #expect(MailDate.epochSeconds("5 Aug 2026 09:00 +0900")
            == MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"))
    }

    /// The same instant written in two zones is one number.
    @Test func theZoneOffsetIsApplied() {
        let jst = MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900")
        #expect(MailDate.epochSeconds("5 Aug 2026 00:00:00 +0000") == jst)
        #expect(MailDate.epochSeconds("4 Aug 2026 19:00:00 -0500") == jst)
    }

    /// **Obsolete and still in the wild.** Reading `26` as year 26
    /// puts the message two thousand years in the past and sorts the
    /// whole list around it.
    @Test func aTwoDigitYearIsReadAsRFC5322Says() {
        #expect(MailDate.epochSeconds("5 Aug 26 09:00:00 +0900")
            == MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"))
        #expect(MailDate.epochSeconds("5 Aug 98 09:00:00 +0900")
            == MailDate.epochSeconds("5 Aug 1998 09:00:00 +0900"))
    }

    @Test func aTrailingCommentIsIgnored() {
        #expect(MailDate.epochSeconds("Tue, 5 Aug 2026 09:00:00 +0900 (JST)")
            == MailDate.epochSeconds("5 Aug 2026 09:00:00 +0900"))
    }

    /// **Nil rather than now.** A message shown as having just arrived
    /// jumps to the top of the list and stays there.
    @Test func anUnreadableDateIsNilRatherThanNow() {
        #expect(MailDate.epochSeconds("") == nil)
        #expect(MailDate.epochSeconds("yesterday") == nil)
        #expect(MailDate.epochSeconds("5 Xxx 2026 09:00:00 +0900") == nil)
        #expect(MailDate.epochSeconds("5 Aug 2026 09:00:00") == nil)
    }

    /// Guessing UTC for an unknown zone is a silent thirteen-hour
    /// error.
    @Test func anUnknownZoneIsRefusedRatherThanGuessedAsUTC() {
        #expect(MailDate.epochSeconds("5 Aug 2026 09:00:00 XYZ") == nil)
        #expect(MailDate.epochSeconds("5 Aug 2026 09:00:00 GMT")
            == MailDate.epochSeconds("5 Aug 2026 09:00:00 +0000"))
    }
}
