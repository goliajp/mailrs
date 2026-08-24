import Foundation
import Testing

@testable import Mailrs

/// The list's derived state.
///
/// `visible` is one window read from the store, not every row sorted in
/// memory — and that is the shape that goes wrong quietly: a derived
/// value somebody has to remember to refresh is one that eventually
/// disagrees with what it was derived from, and disagreeing here means
/// showing the wrong mail. So these assert that it keeps up **without
/// anybody asking it to**.
@Suite @MainActor struct MailboxesModelTests {
    private func row(_ account: String, _ uid: UInt32, date: Int64) -> MailboxRow {
        MailboxRow(
            accountId: account, uid: uid, folder: "INBOX", seen: false,
            sender: "Ada", subject: "Lunch", date: date, messageId: "<\(account)-\(uid)>")
    }

    private func model(_ rows: [MailboxRow]) -> MailboxesModel {
        let model = MailboxesModel()
        AccountStore.replaceRows(rows)
        model.load()
        return model
    }

    @Test func theNewestIsFirst() {
        let model = model([row("a", 1, date: 100), row("a", 2, date: 300)])
        #expect(model.visible.map(\.uid) == [2, 1])
        AccountStore.replaceRows([])
    }

    // Setting the filter must change what is shown, with no refresh
    // call in between — the view sets `only` directly.
    @Test func changingTheFilterChangesWhatIsVisible() {
        let model = model([row("a", 1, date: 100), row("b", 2, date: 300)])
        #expect(model.visible.count == 2)
        model.only = ["a"]
        #expect(model.visible.map(\.accountId) == ["a"])
        model.only = []
        #expect(model.visible.count == 2, "clearing the filter did not restore the list")
        AccountStore.replaceRows([])
    }

    // An empty filter is **no filter**, not "no accounts" — somebody
    // who unticked their last box should see everything again rather
    // than an empty screen.
    @Test func anEmptyFilterIsNoFilter() {
        let model = model([row("a", 1, date: 100)])
        model.only = ["nobody"]
        #expect(model.visible.isEmpty)
        model.only = []
        #expect(model.visible.count == 1)
        AccountStore.replaceRows([])
    }

    // The search runs over the kept order, so it must see rows that
    // arrived after the model was made.
    @Test func aSearchSeesRowsThatArrivedLater() {
        let model = model([row("a", 1, date: 100)])
        model.query = "lunch"
        #expect(model.visible.count == 1)
        model.query = "dinner"
        #expect(model.visible.isEmpty)
        AccountStore.replaceRows([])
    }

    // The window starts at one page and grows on being asked. A list
    // that shows everything at once is the read this replaced.
    @Test func theWindowIsOnePageUntilItIsAskedForMore() {
        let many = (1...(MailboxesModel.page + 10)).map {
            row("a", UInt32($0), date: Int64($0))
        }
        let model = model(many)
        #expect(model.visible.count == MailboxesModel.page)
        #expect(model.moreHeld, "the store had more and did not say so")
        model.showMore()
        #expect(model.visible.count == many.count)
        #expect(!model.moreHeld, "there was nothing more and it said there was")
        AccountStore.replaceRows([])
    }

    // Asking for more when there is none must not spin: the button
    // that fetches from the server is only offered once this stops.
    @Test func showingMoreWhenThereIsNoMoreDoesNothing() {
        let model = model([row("a", 1, date: 100)])
        #expect(!model.moreHeld)
        model.showMore()
        #expect(model.shown == MailboxesModel.page)
        AccountStore.replaceRows([])
    }
}
