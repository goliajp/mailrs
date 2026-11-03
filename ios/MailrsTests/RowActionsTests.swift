import Foundation
import Testing

@testable import Mailrs

/// The action table the web (`lib/actions.ts`) and Android
/// (`wire/RowActions.kt`) share: what each action is, what takes it
/// back, and which ones a row offers where it is shown.
struct RowActionsTests {
    private func row(
        unread: Int = 0, flagged: Bool = false, pinned: Bool = false,
        archived: Bool = false, snoozedUntil: Int64? = nil, trashedAt: Int64? = nil
    ) -> Wire.Conversation {
        Wire.Conversation(
            threadId: "t1", subject: "s", participants: ["a@example.test"], messageCount: 1,
            unreadCount: unread, lastDate: 1, category: "inbox", flagged: flagged,
            snippet: "", pinned: pinned, archived: archived, snoozedUntil: snoozedUntil,
            importanceLevel: "normal", importanceScore: 0, requiresAction: false,
            receivedCount: 1, sentCount: 0, trashedAt: trashedAt)
    }

    @Test func everyUndoTakesItsActionBack() {
        for action in RowAction.allCases {
            guard let back = action.undo, back.undo != nil else { continue }
            // a pair, except the buckets, which all go back to the inbox
            if [.markNotification, .markPromotion].contains(action) {
                #expect(back == .moveToInbox)
                continue
            }
            #expect(back.undo == action, "\(action) undoes to \(back), which undoes to \(String(describing: back.undo))")
        }
    }

    /// Only what cannot be undone asks first.
    @Test func onlyDeletingForGoodAsks() {
        #expect(RowAction.allCases.filter(\.confirm) == [.deleteForever])
        #expect(RowAction.deleteForever.undo == nil)
    }

    /// The verbs `POST /api/conversations/batch` knows, as the server
    /// spells them (`conversation_verbs.rs`). Moving to the Trash is
    /// `delete` there.
    @Test func batchVerbsAreTheServers() {
        let known: Set = [
            "read", "unread", "star", "unstar", "archive", "unarchive", "delete", "untrash",
            "label", "unlabel",
        ]
        for action in RowAction.allCases {
            if let verb = action.batch { #expect(known.contains(verb), "\(action): \(verb)") }
        }
        #expect(RowAction.trash.batch == "delete")
    }

    @Test func anInboxRowOffersTheWebsActionsInItsOrder() {
        let got = RowActions.forRow(row(unread: 1), list: .inbox, inLabel: false, now: 100)
        #expect(got == [
            .read, .star, .pin, .archive, .snooze,
            .markNotification, .markPromotion, .markJunk, .label, .trash,
        ])
    }

    @Test func aRowSaysWhatApplies() {
        let got = RowActions.forRow(
            row(flagged: true, pinned: true, archived: true, snoozedUntil: 200),
            list: .all, inLabel: false, now: 100)
        #expect(got.prefix(5) == [.unread, .unstar, .unpin, .unarchive, .unsnooze])
    }

    @Test func theTrashRestoresOrDeletesForGood() {
        #expect(RowActions.forRow(row(), list: .trash, inLabel: false, now: 0)
            == [.unread, .untrash, .deleteForever])
        // a row in the Trash says so wherever it is shown
        #expect(RowActions.forRow(row(trashedAt: 5), list: .all, inLabel: false, now: 0)
            == [.unread, .untrash, .deleteForever])
    }

    @Test func reviewReleasesOrTrashes() {
        #expect(RowActions.forRow(row(), list: .review, inLabel: false, now: 0) == [.release, .trash])
        #expect(RowActions.forSelection(list: .review, inLabel: false) == [.release, .trash])
    }

    @Test func junkAndNotificationsOfferTheirWayOut() {
        let junk = RowActions.forRow(row(), list: .junk, inLabel: false, now: 0)
        #expect(junk.contains(.markNotJunk) && !junk.contains(.markJunk))
        let np = RowActions.forRow(row(), list: .np, inLabel: false, now: 0)
        #expect(np.contains(.moveToInbox) && !np.contains(.markNotification))
    }

    @Test func aLabelListOffersToTakeTheLabelOff() {
        #expect(RowActions.forRow(row(), list: .label, inLabel: true, now: 0).contains(.unlabel))
        #expect(!RowActions.forRow(row(), list: .inbox, inLabel: false, now: 0).contains(.unlabel))
    }

    /// Each list asks the server for itself: a flag left off reads the
    /// inbox, which shows the wrong rows without an error.
    @Test func theNewListsAskForThemselves() {
        func items(_ list: MailList) -> [String: String] {
            Dictionary(list.axes.queryItems.map { ($0.name, $0.value ?? "") }) { a, _ in a }
        }
        #expect(items(.trash)["trashed"] == "true")
        #expect(items(.pinned)["pinned"] == "true")
        #expect(items(.snoozed)["snoozed"] == "true")
        #expect(items(.all)["folder"] == "all")
        #expect(items(.inbox)["trashed"] == nil)
        #expect(MailListAxes(label: "7").queryItems.contains(URLQueryItem(name: "label", value: "7")))
    }

    @Test func everyActionIsInTheCatalog() throws {
        let app = Bundle(for: Session.self)
        let path = try #require(app.path(forResource: "ja", ofType: "lproj"))
        let ja = try #require(Bundle(path: path))
        for action in RowAction.allCases {
            for key in [action.titleKey, action.doneKey].compactMap({ $0 }) {
                #expect(ja.localizedString(forKey: key, value: key, table: nil) != key,
                        "untranslated: \(key)")
            }
        }
    }

    /// The labels shown in the sidebar are the fixed lists, never the
    /// stand-in for "whichever label is open".
    @Test func theSidebarDrawsEveryListButTheLabelStandIn() {
        #expect(!MailList.shown.contains(.label))
        #expect(MailList.shown.count == MailList.allCases.count - 1)
    }
}
