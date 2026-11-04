import Testing

@testable import Mailrs

/// The same cases as Android's `TeamRowTest`.
struct TeamRowTests {
    private func row(_ assignee: String?) -> Wire.Conversation {
        Wire.Conversation(
            threadId: "t", subject: "s", participants: [], messageCount: 1,
            unreadCount: 0, lastDate: 0, category: "inbox", flagged: false,
            snippet: "", pinned: false, archived: false, snoozedUntil: nil,
            importanceLevel: "normal", importanceScore: 0, requiresAction: false,
            receivedCount: 1, sentCount: 0, assignee: assignee)
    }

    @Test func nobodyAssignedSaysNothing() {
        #expect(TeamRow.assignee(row(nil)) == nil)
        #expect(TeamRow.assignee(row("")) == nil)
        #expect(TeamRow.assignee(row("   ")) == nil)
    }

    @Test func anAddressReadsAsItsLocalPart() {
        #expect(TeamRow.assignee(row("bob@golia.jp")) == "bob")
    }

    @Test func aNameWithoutAnAtIsShownAsIs() {
        #expect(TeamRow.assignee(row("bob")) == "bob")
        #expect(TeamRow.assignee(row("@odd")) == "@odd")
    }
}
