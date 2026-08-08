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

    /// The widths a survey of this mailbox actually found.
    @Test(arguments: [(600.0, 0.61), (640.0, 0.57), (650.0, 0.56),
                      (680.0, 0.54), (700.0, 0.52), (768.0, 0.48)])
    func fitsRealEmailWidthsIntoAPhone(width: Double, approx: Double) {
        let scale = FitToWidth.scale(contentWidth: width, hostWidth: phone)
        #expect(abs(scale - approx) < 0.01)
        #expect(scale > FitToWidth.minScale)
    }

    /// The one the earlier survey missed. 420 messages from a single
    /// newsletter in this mailbox lay out at 1080px, and the old 0.45
    /// floor clamped them — so a quarter of every one of them sat off
    /// the right edge of a body that could not be panned.
    ///
    /// This is the regression test for that: fitting 1080 must produce a
    /// scale that actually fits, not the largest scale someone thought
    /// was still readable.
    @Test func fitsTheWidestNewsletterInTheMailbox() {
        let scale = FitToWidth.scale(contentWidth: 1080, hostWidth: phone)
        #expect(abs(scale - 0.339) < 0.01, "got \(scale)")
        #expect(1080 * scale <= phone + 0.5, "1080 x \(scale) = \(1080 * scale) into \(phone)")
    }

    /// Whatever the width, the result fits — that is the whole contract,
    /// and the property the old floor quietly broke.
    @Test(arguments: [400.0, 600.0, 768.0, 1080.0, 1600.0, 2400.0])
    func anythingUpToThePathologicalActuallyFits(width: Double) {
        let scale = FitToWidth.scale(contentWidth: width, hostWidth: phone)
        #expect(width * scale <= phone + 0.5, "\(width) x \(scale) overflows \(phone)")
    }

    /// The floor still exists, for the genuinely pathological — a
    /// runaway `<pre>` thousands of columns wide. Below it the body is
    /// zoomable, which is what makes small survivable.
    @Test func stopsAtTheFloorRatherThanSmearing() {
        #expect(FitToWidth.scale(contentWidth: 100_000, hostWidth: phone) == FitToWidth.minScale)
    }

    @Test func answersOneWhenItHasNotBeenMeasuredYet() {
        #expect(FitToWidth.scale(contentWidth: 0, hostWidth: phone) == 1)
        #expect(FitToWidth.scale(contentWidth: 600, hostWidth: 0) == 1)
        #expect(FitToWidth.scale(contentWidth: .nan, hostWidth: phone) == 1)
    }
}
