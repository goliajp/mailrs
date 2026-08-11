import Foundation
import Testing

@testable import Mailrs

@Suite("Plain-text links")
struct PlainTextLinksTests {
    /// Every link in the string, as (text, destination).
    private func links(_ text: String) -> [(String, String)] {
        let attributed = PlainTextLinks.attributed(text)
        return attributed.runs.compactMap { run in
            guard let url = run.link else { return nil }
            return (String(attributed[run.range].characters), url.absoluteString)
        }
    }

    @Test("a link with a scheme is followable")
    func schemeIsALink() {
        let found = links("See https://example.com/a?b=1 for details.")
        #expect(found.count == 1)
        #expect(found.first?.0 == "https://example.com/a?b=1")
        #expect(found.first?.1 == "https://example.com/a?b=1")
    }

    /// A sentence that ends after a link must not swallow the full stop.
    @Test("trailing punctuation stays out of it")
    func trailingPunctuation() {
        #expect(links("Read https://example.com/a. Then reply.").first?.0
            == "https://example.com/a")
        #expect(links("In parens (https://example.com/a) ok").first?.0
            == "https://example.com/a")
    }

    /// 146 bare addresses in 400 real bodies. In a mail client the
    /// obvious thing to do with one is write to it.
    @Test("a bare address becomes mailto")
    func addressBecomesMailto() {
        let found = links("Mail me at someone@example.com please")
        #expect(found.first?.1 == "mailto:someone@example.com")
    }

    /// The measurement this rule came from: 15 bare hostnames in 400
    /// real bodies, and the set contains the name of a technology and
    /// the name of a shop. Linkifying those teaches a reader that the
    /// underlining means nothing.
    @Test("a bare hostname in prose is prose")
    func bareHostIsNotALink() {
        #expect(links("Built on ASP.NET and shipped").isEmpty)
        #expect(links("Ordered from Amazon.co.jp last week").isEmpty)
        #expect(links("Bare host example.com nope?").isEmpty)
    }

    /// One real body carries `insightapp://register?…`. A deep link into
    /// another app is the shape a phishing message takes when it wants
    /// out of the browser, and the HTML body already refuses these.
    @Test("a custom scheme is not followed")
    func customSchemeRefused() {
        #expect(links("Open insightapp://register?token=vt-a7b4 now").isEmpty)
        #expect(!PlainTextLinks.followable(
            matched: "insightapp://register", url: URL(string: "insightapp://register")!))
        #expect(!PlainTextLinks.followable(
            matched: "file:///etc/passwd", url: URL(string: "file:///etc/passwd")!))
        #expect(PlainTextLinks.followable(
            matched: "https://x.example", url: URL(string: "https://x.example")!))
        #expect(PlainTextLinks.followable(
            matched: "a@x.example", url: URL(string: "mailto:a@x.example")!))
    }

    /// The hole the measurement-driven test found: given `ASP.NET` the
    /// detector hands back a URL of `http://ASP.NET`, so a check that
    /// reads the URL's scheme sees `http` and waves it through. The text
    /// is what has to be asked.
    @Test("a synthesised scheme does not count as one")
    func synthesisedSchemeRejected() {
        #expect(!PlainTextLinks.followable(
            matched: "ASP.NET", url: URL(string: "http://ASP.NET")!))
        #expect(PlainTextLinks.followable(
            matched: "http://real.example", url: URL(string: "http://real.example")!))
    }

    @Test("several links in one body all resolve")
    func severalLinks() {
        let found = links("Two https://a.example and https://b.example here")
        #expect(found.map(\.1) == ["https://a.example", "https://b.example"])
    }

    /// `range(of:)` would have found the first occurrence twice, leaving
    /// the second copy dead and the first one underlined twice.
    @Test("two identical links are two links")
    func identicalLinksAreDistinct() {
        let found = links("https://x.example/a and again https://x.example/a")
        #expect(found.count == 2)
        #expect(found.allSatisfy { $0.1 == "https://x.example/a" })
    }

    /// Three index spaces meet here: the detector counts UTF-16 units,
    /// `AttributedString` counts characters, and an emoji is two of the
    /// first and one of the second. Getting this wrong underlines the
    /// characters after the link instead of the link.
    @Test("an emoji before the link does not shift it")
    func emojiDoesNotShiftTheRange() {
        let found = links("🎉🎉 see https://example.com/a now")
        #expect(found.first?.0 == "https://example.com/a")
    }

    /// Real mail is full of CJK, and a Japanese path is a valid URL.
    @Test("a CJK path survives")
    func cjkPath() {
        let found = links("詳しくは https://example.jp/お知らせ をご覧ください")
        #expect(found.count == 1)
        #expect(found.first?.0 == "https://example.jp/お知らせ")
    }

    @Test("text with no link is unchanged")
    func noLinks() {
        let text = "Just a sentence with nothing to follow."
        #expect(links(text).isEmpty)
        #expect(String(PlainTextLinks.attributed(text).characters) == text)
    }

    @Test("an empty body does not crash")
    func emptyBody() {
        #expect(String(PlainTextLinks.attributed("").characters).isEmpty)
    }
}

@Suite("Plain text carries more than links")
struct PlainTextDetectionTests {
    /// A URL that happens to contain a phone-shaped run is a URL. The
    /// two detectors run over the same text without knowing about each
    /// other, so the link claim has to win.
    @Test("a link is never overwritten by a number inside it")
    func linkWins() {
        let text = "See https://example.com/order/03-3964-2611 for details"
        let attributed = PlainTextLinks.attributed(text)
        let links = attributed.runs.compactMap(\.link?.absoluteString)
        #expect(links.allSatisfy { $0.hasPrefix("https://") }, "got \(links)")
    }

    @Test("a phone number in prose becomes dialable")
    func numberInProse() {
        let attributed = PlainTextLinks.attributed("Reception is on 03-3964-2611.")
        let links = attributed.runs.compactMap(\.link?.absoluteString)
        #expect(links == ["tel:0339642611"])
    }
}
