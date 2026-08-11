import Foundation
import Testing

@testable import Mailrs

@Suite("What a body offers to act on")
struct BodyDetectionsTests {
    private func urls(_ text: String) -> [String] {
        BodyDetections.hits(in: text).map(\.url.absoluteString)
    }

    /// The numbers in the sample that were real, in the shapes they
    /// were written in.
    @Test("a number written to be dialled is dialable")
    func realNumbers() {
        for number in ["080-5654-6595", "03-3964-2611", "0120-23-28-86", "+1 888-303-0108"] {
            #expect(BodyDetections.dialable(number), "\(number) is a phone number")
        }
    }

    /// And the ones that were not. `4300078149` came out of an order
    /// confirmation eight times in one message; a client offering to
    /// call it is a client that cannot be trusted to know what it is
    /// looking at.
    @Test("a bare run of digits is a reference number, not a phone")
    func referenceNumbers() {
        for number in ["4300078149", "856008662291", "992607637761"] {
            #expect(!BodyDetections.dialable(number), "\(number) is not a phone number")
        }
    }

    @Test("a phone number becomes a tel: URL")
    func telLink() {
        let found = urls("Call us on 03-3964-2611 any weekday.")
        #expect(found == ["tel:0339642611"])
    }

    /// A conference bridge is useless without its pauses — `,` waits,
    /// `#` sends.
    @Test("an extension survives into the dialled string")
    func extensionSurvives() {
        let found = BodyDetections.telURL("+91 40 6480 2661,,,,886171933#")
        #expect(found?.absoluteString == "tel:+914064802661,,,,886171933#")
    }

    /// 42% of real bodies carry one, and Japanese postal addresses are
    /// the common case in this mailbox.
    @Test("a postal address opens in Maps")
    func addressLink() {
        let found = urls("〒100-0005 東京都千代田区丸の内1-8-1 におこしください")
        #expect(found.count == 1, "expected one address, got \(found)")
        #expect(found.first?.hasPrefix("https://maps.apple.com/?q=") == true)
    }

    /// Dates are the most common thing in the corpus and deliberately
    /// not detected: `NSDataDetector` calls the single character 今 a
    /// date, so ordinary prose would come back speckled with tappable
    /// nothing.
    @Test("dates are left alone")
    func datesAreNotTouched() {
        #expect(urls("今 の状況を 2026-06-06 までにお知らせください").isEmpty)
    }

    @Test("prose with nothing to act on offers nothing")
    func nothingToOffer() {
        #expect(urls("Thanks — that all sounds right to me.").isEmpty)
        #expect(urls("").isEmpty)
    }
}
