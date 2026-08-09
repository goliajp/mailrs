import Foundation

/// Links in a plain-text message body.
///
/// 70% of the plain-text mail in the real mailbox (1,271 of 1,804) has a
/// URL in it, and this client rendered every one of them with
/// `Text(verbatim:)` — dead characters you could read and not follow.
///
/// `NSDataDetector` finds them, and the whole design question is what to
/// accept from it. Over 400 real bodies it returned 1,156 links carrying
/// a scheme, 146 bare email addresses, no `www.`-prefixed hosts at all,
/// and 15 bare hostnames — a set that includes **`ASP.NET`**,
/// `Amazon.co.jp` and `Amazon.com`, which are the name of a technology
/// and the name of a shop written in a sentence, not places anyone meant
/// to send a reader.
///
/// So: a scheme, or an address. That drops one real shortened link
/// (`bit.ly/4hN4ox5`, written without `https://`) and it drops
/// `ASP.NET`, and between those two the second is the one that must not
/// happen — a client that turns prose into links teaches people that its
/// links mean nothing.
///
/// Foundation only, no SwiftUI: how a link *looks* is the view's
/// business, and keeping this side pure is what lets the rule be tested
/// without a screen.
enum PlainTextLinks {
    private static let detector = try? NSDataDetector(
        types: NSTextCheckingResult.CheckingType.link.rawValue)

    /// The body, with `http`, `https` and `mailto` targets made
    /// followable and everything else left as the text it is.
    static func attributed(_ text: String) -> AttributedString {
        var result = AttributedString(text)
        guard let detector, !text.isEmpty else { return result }
        let whole = NSRange(text.startIndex..<text.endIndex, in: text)
        for match in detector.matches(in: text, range: whole) {
            guard let url = match.url,
                  let found = Range(match.range, in: text),
                  followable(matched: String(text[found]), url: url),
                  let range = attributedRange(of: match.range, in: text, within: result)
            else { continue }
            result[range].link = url
        }
        return result
    }

    /// Only what this client is willing to open.
    ///
    /// Judged on the **matched text**, not on the URL the detector
    /// produced, and the difference is the whole rule: given the prose
    /// `ASP.NET` the detector hands back a URL of `http://ASP.NET`, so
    /// anything that asks the URL for its scheme is told `http` and lets
    /// it through. The first version did exactly that, and all three
    /// cases the measurement was built around passed as links.
    ///
    /// A bare address is the one thing allowed to gain a scheme it did
    /// not have: `someone@example.com` becomes `mailto:`, which is what
    /// a mail client should do with one.
    ///
    /// A custom scheme — one real body carries `insightapp://register?…`
    /// — is a deep link into some other app, which is the shape a
    /// phishing message takes when it wants to leave the browser behind.
    /// The HTML body already refuses to navigate anywhere but http and
    /// https (`WebNavigation`), and plain text answering differently
    /// would be a hole in the same wall.
    static func followable(matched text: String, url: URL) -> Bool {
        let lower = text.lowercased()
        if lower.hasPrefix("http://") || lower.hasPrefix("https://") { return true }
        return url.scheme?.lowercased() == "mailto"
    }

    /// The detector's UTF-16 offsets, as somewhere in the
    /// `AttributedString`.
    ///
    /// Three index spaces, and going straight from one to the third is
    /// wrong: `NSRange` counts UTF-16 units, `AttributedString` offsets
    /// count characters, and an emoji is two of the first and one of the
    /// second. `Range(_:in:)` crosses the first gap correctly, and
    /// `distance(from:to:)` measures the result in characters — so a
    /// body with an emoji before its link still underlines the link and
    /// not the four characters after it.
    ///
    /// Not `range(of:)` on the text either: two identical links in one
    /// message would both resolve to the first.
    private static func attributedRange(
        of nsRange: NSRange, in text: String, within attributed: AttributedString
    ) -> Range<AttributedString.Index>? {
        guard let found = Range(nsRange, in: text) else { return nil }
        let offset = text.distance(from: text.startIndex, to: found.lowerBound)
        let length = text.distance(from: found.lowerBound, to: found.upperBound)
        let start = attributed.index(attributed.startIndex, offsetByCharacters: offset)
        guard start < attributed.endIndex else { return nil }
        let end = attributed.index(start, offsetByCharacters: length)
        guard end <= attributed.endIndex else { return nil }
        return start..<end
    }
}
