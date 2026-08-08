import CoreGraphics
import Testing

@testable import Mailrs

/// The parser exists so the icon data can stay verbatim upstream, so
/// what it has to get right is upstream's actual syntax — relative
/// commands, arcs, implicit repeats, and numbers written without
/// separators.
struct SVGPathTests {
    private func bounds(_ d: String) -> CGRect {
        SVGPath.parse(d).boundingRect
    }

    @Test func absoluteMoveAndLine() {
        let box = bounds("M2 12h20")
        #expect(box.minX == 2)
        #expect(box.maxX == 22)
        #expect(box.height == 0)
    }

    /// `m15 9 6-6` — relative, and `6-6` is two numbers with no
    /// separator between them, which is how Lucide writes it.
    @Test func relativeCommandsAndUnseparatedNumbers() {
        let box = bounds("m15 9 6-6")
        #expect(box.minX == 15)
        #expect(box.maxX == 21)
        #expect(box.minY == 3)
        #expect(box.maxY == 9)
    }

    /// A repeated argument list continues the previous command, and a
    /// repeated `m` continues as `l` rather than moving again.
    @Test func anArgumentListRepeatsItsCommand() {
        let box = bounds("M0 0 10 0 10 10")
        #expect(box.width == 10)
        #expect(box.height == 10)
    }

    /// Two half-arcs make a circle: the `users` avatar is drawn that
    /// way, and getting the sweep flag wrong turns it inside out.
    @Test func arcsCloseIntoTheCircleTheyDescribe() {
        let box = bounds("M4 12a8 8 0 1 0 16 0a8 8 0 1 0-16 0")
        #expect(abs(box.minX - 4) < 0.01, "left edge \(box.minX)")
        #expect(abs(box.maxX - 20) < 0.01, "right edge \(box.maxX)")
        #expect(abs(box.minY - 4) < 0.01, "top edge \(box.minY)")
        #expect(abs(box.maxY - 20) < 0.01, "bottom edge \(box.maxY)")
    }

    /// Radii too small to span the endpoints are scaled up, per the
    /// spec. Without it the arc silently disappears.
    @Test func undersizedRadiiAreScaledRatherThanDropped() {
        let box = bounds("M0 0a1 1 0 0 0 10 0")
        #expect(box.width > 9, "expected the arc to span its endpoints, got \(box.width)")
    }

    /// The real thing: one of Lucide's own paths, arcs and all.
    @Test func aRealLucidePathStaysInsideTheGrid() {
        let box = bounds("M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2")
        #expect(box.minX >= 1.9 && box.maxX <= 16.1, "x \(box.minX)…\(box.maxX)")
        #expect(box.minY >= 14.9 && box.maxY <= 21.1, "y \(box.minY)…\(box.maxY)")
    }

    /// Every icon actually shipped parses to something with area — the
    /// cheapest guard against a paste that lost a character.
    @Test func everyShippedIconDraws() {
        let icons: [(String, [Lucide.Element])] = [
            ("users", Lucide.users), ("split", Lucide.split), ("mails", Lucide.mails),
            ("globe", Lucide.globe), ("lockKeyhole", Lucide.lockKeyhole),
            ("send", Lucide.send), ("shieldCheck", Lucide.shieldCheck),
            ("scrollText", Lucide.scrollText), ("keyRound", Lucide.keyRound),
        ]
        for (name, elements) in icons {
            for element in elements {
                let box = LucideIcon.path(for: element).boundingRect
                // Not `isEmpty`: a `CGRect` of zero height is "empty",
                // and three of these elements are horizontal rules —
                // `M2 12h20` in globe, two more in scroll-text. A line
                // is a drawing. What must not happen is *no* extent.
                #expect(box.width + box.height > 0, "\(name) drew nothing")
                #expect(!box.isInfinite && !box.isNull, "\(name) is unbounded")
                // Lucide's grid is 24×24; anything outside it is a
                // mis-paste, not a drawing.
                #expect(box.minX >= -0.5 && box.maxX <= 24.5, "\(name) x \(box.minX)…\(box.maxX)")
                #expect(box.minY >= -0.5 && box.maxY <= 24.5, "\(name) y \(box.minY)…\(box.maxY)")
            }
        }
    }

    @Test func nonsenseDoesNotCrash() {
        #expect(SVGPath.parse("").isEmpty)
        #expect(SVGPath.parse("M").isEmpty)
        _ = SVGPath.parse("M0 0 L")
        _ = SVGPath.parse("zzzz")
    }
}
