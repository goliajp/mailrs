import Testing

@testable import Mailrs

struct AlignmentRateTests {
    @Test func nothingCountedIsNotZeroPercent() {
        #expect(AlignmentRate.fraction(passing: 0, total: 0) == nil)
        #expect(AlignmentRate.percentText(passing: 0, total: 0) == nil)
    }

    @Test func everythingAlignedIsAHundred() {
        #expect(AlignmentRate.percentText(passing: 40, total: 40) == "100.0%")
    }

    /// Almost-perfect must not read as perfect: a screen that rounded
    /// 9,994 of 10,000 up to 100% would hide six rejected messages.
    @Test func almostPerfectDoesNotRoundToPerfect() {
        #expect(AlignmentRate.percentText(passing: 9_994, total: 10_000) == "99.9%")
    }

    @Test func nothingAlignedIsZero() {
        #expect(AlignmentRate.percentText(passing: 0, total: 12) == "0.0%")
    }
}
