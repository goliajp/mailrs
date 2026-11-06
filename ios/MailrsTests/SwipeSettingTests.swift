import Foundation
import Testing

@testable import Mailrs

/// The swipe setting: its choices and default against `actions.json`'s
/// `swipe` block (the server is checked against the same block), and the
/// action each choice runs on a row.
struct SwipeSettingTests {
    private func row(unread: Int = 0, archived: Bool = false, trashedAt: Int64? = nil) -> Wire.Conversation {
        Wire.Conversation(
            threadId: "t1", subject: "s", participants: ["a@example.test"], messageCount: 1,
            unreadCount: unread, lastDate: 1, category: "inbox", flagged: false,
            snippet: "", pinned: false, archived: archived, snoozedUntil: nil,
            importanceLevel: "normal", importanceScore: 0, requiresAction: false,
            receivedCount: 1, sentCount: 0, trashedAt: trashedAt)
    }

    private struct Block: Decodable {
        struct Swipe: Decodable {
            let choices: [String]
            let `default`: [String: String]
        }
        let swipe: Swipe
    }

    @Test func choicesAndDefaultAreTheSharedTables() throws {
        let url = try #require(Bundle.main.url(forResource: "actions", withExtension: "json"))
        let table = try JSONDecoder().decode(Block.self, from: Data(contentsOf: url)).swipe
        #expect(SwipeChoice.allCases.map(\.rawValue) == table.choices)
        #expect(SwipeSetting.standard.left.rawValue == table.default["left"])
        #expect(SwipeSetting.standard.right.rawValue == table.default["right"])
    }

    @Test func theDefaultArchivesRightAndTrashesLeft() {
        #expect(SwipeSetting.standard.right.action(for: row(), in: .inbox) == .archive)
        #expect(SwipeSetting.standard.left.action(for: row(), in: .inbox) == .trash)
    }

    @Test func eachChoiceRunsTheActionThatApplies() {
        #expect(SwipeChoice.archive.action(for: row(archived: true), in: .archived) == .unarchive)
        #expect(SwipeChoice.read.action(for: row(unread: 2), in: .inbox) == .read)
        #expect(SwipeChoice.read.action(for: row(), in: .inbox) == .unread)
        #expect(SwipeChoice.none.action(for: row(), in: .inbox) == nil)
    }

    /// Nothing a swipe runs deletes for good.
    @Test func inTheTrashBothRestore() {
        let trashed = row(trashedAt: 5)
        for choice in [SwipeChoice.archive, .trash] {
            #expect(choice.action(for: trashed, in: .trash) == .untrash)
            #expect(choice.action(for: row(), in: .trash) == .untrash)
        }
        for choice in SwipeChoice.allCases {
            #expect(choice.action(for: trashed, in: .trash) != .deleteForever)
        }
    }

    @Test func theWireNamesAreTheServers() throws {
        let s = try JSONDecoder().decode(SwipeSetting.self, from: Data(#"{"left":"none","right":"read"}"#.utf8))
        #expect(s == SwipeSetting(left: .none, right: .read))
    }
}
