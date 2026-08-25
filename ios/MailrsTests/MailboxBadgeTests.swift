import Testing

@testable import Mailrs

/// Which sidebar row carries a number.
///
/// The server gives one unseen total, not a count per mailbox. Putting
/// it beside every row would print the same number seven times and say
/// "four unread in Archived" — information that is not merely missing
/// but wrong.
@Suite struct MailboxBadgeTests {
    @Test func theOpenListCarriesTheCount() {
        #expect(MailList.inbox.badgeCount(activeList: .inbox, unreadInActive: 4) == 4)
    }

    @Test func everyOtherListCarriesNothing() {
        for list in MailList.allCases where list != .inbox {
            #expect(
                list.badgeCount(activeList: .inbox, unreadInActive: 4) == nil,
                "\(list.rawValue) claimed a count it cannot know")
        }
    }

    // Zero is nothing, not a badge reading "0" — an empty badge takes
    // the space of a real one and says less than no badge at all.
    @Test func zeroIsNoBadge() {
        #expect(MailList.inbox.badgeCount(activeList: .inbox, unreadInActive: 0) == nil)
    }
}
