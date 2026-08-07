import Testing

@testable import Mailrs

struct ThreadCollapseTests {
    /// The newest message is the reason you came; the rest are context.
    @Test func onlyTheLastStartsExpanded() {
        #expect(ThreadCollapse.isExpanded(uid: 2, lastUid: 2, toggled: []))
        #expect(!ThreadCollapse.isExpanded(uid: 1, lastUid: 2, toggled: []))
    }

    /// Toggling works both directions: an older message opens, the
    /// newest can be folded away.
    @Test func togglingInvertsEitherDirection() {
        #expect(ThreadCollapse.isExpanded(uid: 1, lastUid: 2, toggled: [1]))
        #expect(!ThreadCollapse.isExpanded(uid: 2, lastUid: 2, toggled: [2]))
    }

    @Test func aSingleMessageThreadIsExpanded() {
        #expect(ThreadCollapse.isExpanded(uid: 7, lastUid: 7, toggled: []))
    }

    @Test func snippetsFoldWhitespaceAndBound() {
        #expect(ThreadCollapse.snippet("  a\n\n  b\tc  ") == "a b c")
        #expect(ThreadCollapse.snippet(nil) == "")
        let long = String(repeating: "x", count: 200)
        let cut = ThreadCollapse.snippet(long)
        #expect(cut.count == 81 && cut.hasSuffix("…"))
    }
}
