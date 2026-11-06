import Foundation
import Testing

@testable import Mailrs

/// The sectioned Inbox: what the list asks for, and whom "Accept all" accepts.
struct InboxSectionsTests {
    private func row(_ participants: [String]) -> Wire.Conversation {
        Wire.Conversation(
            threadId: "t", subject: "s", participants: participants, messageCount: 1,
            unreadCount: 0, lastDate: 0, category: "inbox", flagged: false,
            snippet: "", pinned: false, archived: false, snoozedUntil: nil,
            importanceLevel: "normal", importanceScore: 0, requiresAction: false,
            receivedCount: 1, sentCount: 0)
    }

    @Test func aSectionIsAskedForByName() {
        var axes = MailList.inbox.axes
        axes.section = InboxSection.awaiting.rawValue
        #expect(axes.queryItems.contains(URLQueryItem(name: "section", value: "awaiting")))
    }

    /// Review is listed from its own endpoint; a search typed into it
    /// must still search it rather than every other list.
    @Test func aSearchInReviewNamesReview() {
        #expect(MailList.review.axes.queryItems.contains(URLQueryItem(name: "quarantined", value: "true")))
        #expect(!MailList.inbox.axes.queryItems.contains { $0.name == "quarantined" })
    }

    @Test func theWholeInboxSendsNoSection() {
        #expect(!MailList.inbox.axes.queryItems.contains { $0.name == "section" })
    }

    /// Each row's last participant who is not the reader, once each.
    @Test func acceptTakesEachRowsLatestOtherSender() {
        let rows = [
            row(["Ann <ANN@x.test>", "me@golia.jp"]),
            row(["bob@y.test"]),
            row(["Ann <ann@x.test>"]),
            row(["me@golia.jp"]),
        ]
        #expect(InboxSection.latestSenders(rows, me: "Me@golia.jp") == ["ann@x.test", "bob@y.test"])
    }
}
