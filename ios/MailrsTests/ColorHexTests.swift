import SwiftUI
import XCTest

@testable import Mailrs

/// The account dot is the same colour on all three clients because the
/// server chose it, so reading it must not be approximate.
final class ColorHexTests: XCTestCase {
    private func rgb(_ c: Color) -> (Double, Double, Double) {
        #if canImport(UIKit)
            var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
            UIColor(c).getRed(&r, green: &g, blue: &b, alpha: &a)
            return (Double(r), Double(g), Double(b))
        #else
            return (0, 0, 0)
        #endif
    }

    func testAHexColourIsReadExactly() {
        let (r, g, b) = rgb(Color(hex: "#22c55e"))
        XCTAssertEqual(r, 34.0 / 255, accuracy: 0.01)
        XCTAssertEqual(g, 197.0 / 255, accuracy: 0.01)
        XCTAssertEqual(b, 94.0 / 255, accuracy: 0.01)
    }

    func testTheLeadingHashIsOptional() {
        XCTAssertEqual(rgb(Color(hex: "22c55e")).0, rgb(Color(hex: "#22c55e")).0, accuracy: 0.001)
    }

    /// A colour that will not parse must not take the row down with
    /// it — and a row with no dot reads as a different kind of account,
    /// so there is always one.
    func testNonsenseFallsBackToGreyRatherThanCrashing() {
        for junk in ["", "#", "nope", "#12345", "#1234567", "#gggggg"] {
            let (r, g, b) = rgb(Color(hex: junk))
            XCTAssertEqual(r, g, accuracy: 0.1, "\(junk) was not grey")
            XCTAssertEqual(g, b, accuracy: 0.1, "\(junk) was not grey")
        }
    }
}
