import Foundation
import Testing

@testable import Mailrs

/// Filing a message, on servers that differ about how.
///
/// The assertion that earns its place is the last one: **a server
/// without UIDPLUS is never sent a bare EXPUNGE**, because that would
/// remove every message in the folder somebody else's client had
/// flagged — mail this app never saw and cannot bring back.
@Suite struct MovePlanTests {
    private func texts(_ caps: Set<String>) -> [String] {
        MovePlan.steps(uid: 7, folder: "Trash", capabilities: caps).compactMap {
            switch $0 {
            case .command(let text): return text
            case .markDeleted: return nil
            }
        }
    }

    @Test func aServerWithMoveIsAskedOnce() {
        #expect(texts(["MOVE", "UIDPLUS"]) == ["UID MOVE 7 \"Trash\""])
    }

    @Test func withoutMoveTheMessageIsCopiedThenFlagged() {
        let steps = MovePlan.steps(uid: 7, folder: "Trash", capabilities: ["UIDPLUS"])
        #expect(
            steps == [
                .command("UID COPY 7 \"Trash\""), .markDeleted, .command("UID EXPUNGE 7"),
            ])
    }

    // **The one that matters.** A bare EXPUNGE takes every \Deleted
    // message in the folder, including ones another client flagged and
    // has not expunged. Flagged-and-left disappears from the list just
    // the same and takes nothing with it.
    @Test func aServerWithoutUidplusIsNeverSentAnExpunge() {
        let plain = texts([])
        #expect(plain.contains { $0.hasPrefix("UID COPY") }, "the copy was not made")
        #expect(!plain.contains { $0.contains("EXPUNGE") }, "an expunge was sent")
        #expect(
            MovePlan.steps(uid: 7, folder: "Trash", capabilities: []).contains(.markDeleted),
            "the message was not flagged, so it would stay in both folders")
    }

    // A folder name with a space is a name, not syntax.
    @Test func anAwkwardFolderNameIsQuoted() {
        #expect(
            MovePlan.steps(uid: 1, folder: "[Gmail]/All Mail", capabilities: ["MOVE"])
                == [.command("UID MOVE 1 \"[Gmail]/All Mail\"")])
    }
}
