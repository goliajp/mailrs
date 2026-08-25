import SwiftUI
import Testing

@testable import Mailrs

/// Which design a scene gets.
@Suite struct PadLayoutTests {
    @Test func regularWidthSplits() {
        #expect(PadLayout.splits(.regular))
    }

    // A phone, and an iPad in Slide Over — both are one column of
    // content, and three panes in that width is three cramped columns.
    @Test func compactWidthDoesNot() {
        #expect(!PadLayout.splits(.compact))
    }

    // Before SwiftUI has resolved a size class. A split view that
    // appears and then collapses is worse than one that appears a
    // frame late.
    @Test func unresolvedIsTreatedAsCompact() {
        #expect(!PadLayout.splits(nil))
    }
}
