import Testing

@testable import Mailrs

/// The same cases `web/src/lib/__tests__/fit-to-width.test.ts` asserts.
///
/// Deliberately duplicated rather than shared: there is no way to run one
/// test against both a TypeScript and a Swift implementation, so the next
/// best thing is that the two suites read alike and a divergence shows up
/// as a diff between two files that are meant to match.
struct FitToWidthTests {
    /// 390pt phone less the body's padding.
    let phone = 366.0
    let desktop = 820.0

    @Test func leavesAMessageThatAlreadyFitsAlone() {
        #expect(FitToWidth.scale(contentWidth: 600, hostWidth: desktop) == 1)
        #expect(FitToWidth.scale(contentWidth: 366, hostWidth: phone) == 1)
    }

    @Test func neverEnlargesANarrowMessage() {
        #expect(FitToWidth.scale(contentWidth: 480, hostWidth: desktop) == 1)
    }

    /// The widths a survey of this mailbox actually found. Every one fits
    /// without touching the floor — the property the floor was chosen for.
    @Test(arguments: [(600.0, 0.61), (640.0, 0.57), (650.0, 0.56),
                      (680.0, 0.54), (700.0, 0.52), (768.0, 0.48)])
    func fitsRealEmailWidthsIntoAPhone(width: Double, approx: Double) {
        let scale = FitToWidth.scale(contentWidth: width, hostWidth: phone)
        #expect(abs(scale - approx) < 0.01)
        #expect(scale > FitToWidth.minScale)
    }

    @Test func stopsAtTheFloorRatherThanSmearing() {
        #expect(FitToWidth.scale(contentWidth: 3000, hostWidth: phone) == FitToWidth.minScale)
    }

    @Test func answersOneWhenItHasNotBeenMeasuredYet() {
        #expect(FitToWidth.scale(contentWidth: 0, hostWidth: phone) == 1)
        #expect(FitToWidth.scale(contentWidth: 600, hostWidth: 0) == 1)
        #expect(FitToWidth.scale(contentWidth: .nan, hostWidth: phone) == 1)
    }
}
