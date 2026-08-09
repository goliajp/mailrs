import SwiftUI
import Testing

@testable import Mailrs

@Suite("Row layout at the reader's text size")
struct RowLayoutTests {
    private let ordinary: [DynamicTypeSize] = [
        .xSmall, .small, .medium, .large, .xLarge, .xxLarge, .xxxLarge,
    ]
    private let accessibility: [DynamicTypeSize] = [
        .accessibility1, .accessibility2, .accessibility3, .accessibility4, .accessibility5,
    ]

    /// Below the accessibility sizes the row is two tight lines, which is
    /// what makes a long list scannable.
    @Test("ordinary sizes keep the compact row")
    func compactBelowAccessibility() {
        for size in ordinary {
            #expect(!RowLayout.stacksHeader(size), "\(size)")
            #expect(RowLayout.senderLines(size) == 1, "\(size)")
            #expect(RowLayout.subjectLines(size) == 1, "\(size)")
            #expect(RowLayout.recipientLines(size) == 1, "\(size)")
            #expect(RowLayout.threadSubjectLines(size) == 3, "\(size)")
            #expect(RowLayout.gutterAlignment(size) == .center, "\(size)")
        }
    }

    /// The measurement this came from: at
    /// `accessibility-extra-extra-extra-large` the sender rendered as
    /// "A…" while the date kept its full width, and the subject came out
    /// as "Quarterly r…" — with half the screen empty below.
    @Test("accessibility sizes stack the header and let the text breathe")
    func stackedAtAccessibility() {
        for size in accessibility {
            #expect(RowLayout.stacksHeader(size), "\(size)")
            #expect(RowLayout.senderLines(size) == 2, "\(size)")
            #expect(RowLayout.subjectLines(size) == 3, "\(size)")
            #expect(RowLayout.recipientLines(size) == 3, "\(size)")
            #expect(RowLayout.threadSubjectLines(size) == 6, "\(size)")
            #expect(RowLayout.gutterAlignment(size) == .top, "\(size)")
        }
    }

    /// `xxxLarge` is the largest *ordinary* size and `accessibility1` the
    /// smallest accessibility one. Getting that boundary wrong by one
    /// either wastes the fix or applies it to a size that never needed
    /// it.
    @Test("the boundary is where iOS puts it")
    func theBoundary() {
        #expect(!RowLayout.stacksHeader(.xxxLarge))
        #expect(RowLayout.stacksHeader(.accessibility1))
    }
}
