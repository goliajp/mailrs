import Foundation

/// The things in a message body that are worth a tap.
///
/// Links were already followable; this is the rest of what a reader
/// points at. What to detect was measured over 897 real message bodies
/// from the production mailbox rather than guessed:
///
/// | what              | bodies carrying one | decision |
/// |-------------------|--------------------:|----------|
/// | a postal address  |          379 (42%)  | detect   |
/// | a phone number    |           92 (10%)  | detect, filtered |
/// | a date            |          631 (70%)  | **no** — see below |
/// | a verification code |           ~3      | **no** — see below |
///
/// **Dates are not detected**, despite being the most common thing on
/// the list. `NSDataDetector` returns `今` — the single character for
/// "now" — as a date, and 1,367 matches across 897 bodies means a page
/// of ordinary Japanese prose would come back speckled with tappable
/// nothing. A date is also the one item here with no unambiguous
/// action: a phone number dials, an address opens in Maps, and a date
/// asks *which calendar, whose calendar, how long*.
///
/// **Verification codes are not detected.** Twelve of 897 bodies match
/// a code-shaped pattern and a hand count of them found roughly three
/// real one-time codes; the rest were "verify the setting status",
/// "postcode patterns for Colombia", and an RSVP code. A Copy Code
/// button that appears on the wrong number, or fails to appear on the
/// right one, is worse than no button — the reader has to check it
/// either way, and then it has cost them a glance for nothing.
enum BodyDetections {
    /// One thing found in the text, and what tapping it should do.
    struct Hit: Equatable {
        let range: NSRange
        let url: URL
    }

    private static let detector = try? NSDataDetector(
        types: NSTextCheckingResult.CheckingType.phoneNumber.rawValue
            | NSTextCheckingResult.CheckingType.address.rawValue)

    /// Every dialable number and postal address in `text`.
    static func hits(in text: String) -> [Hit] {
        guard let detector, !text.isEmpty else { return [] }
        let whole = NSRange(text.startIndex..<text.endIndex, in: text)
        var out: [Hit] = []
        for match in detector.matches(in: text, range: whole) {
            guard let found = Range(match.range, in: text) else { continue }
            let matched = String(text[found])
            switch match.resultType {
            case .phoneNumber:
                guard dialable(matched), let url = telURL(matched) else { continue }
                out.append(Hit(range: match.range, url: url))
            case .address:
                guard let url = mapsURL(matched) else { continue }
                out.append(Hit(range: match.range, url: url))
            default:
                continue
            }
        }
        return out
    }

    /// A number someone could dial, as written.
    ///
    /// A bare run of digits is an invoice number as often as a phone
    /// number, and the detector cannot tell them apart — it returned
    /// `4300078149` from an order confirmation eight times in one
    /// message. What a number meant for dialling carries is
    /// punctuation or a country code: `080-5654-6595`,
    /// `+91 40 6480 2661`, `0120-23-28-86`.
    ///
    /// Over the 897-body sample this keeps 164 matches and drops 83.
    /// Every kept sample inspected was a real number and every dropped
    /// one was a reference number, which is the trade this makes: a
    /// phone number nobody can tap is a mild loss, and an order number
    /// that offers to call it is a client that cannot be trusted to
    /// know what it is looking at.
    static func dialable(_ text: String) -> Bool {
        if text.hasPrefix("+") { return true }
        return text.contains("-") || text.contains(" ") || text.contains("(")
    }

    /// `tel:` with everything a dialler cannot use taken out.
    ///
    /// Pause and wait characters survive — `,` and `;` are how a number
    /// carries an extension, and the conference bridge in the sample
    /// (`+91 40 6480 2661,,,,886171933#`) is useless without them.
    static func telURL(_ text: String) -> URL? {
        let allowed = CharacterSet(charactersIn: "+0123456789,;#*")
        let digits = String(text.unicodeScalars.filter { allowed.contains($0) })
        guard digits.count >= 3 else { return nil }
        return URL(string: "tel:\(digits)")
    }

    /// Apple Maps, with the address as the query.
    static func mapsURL(_ text: String) -> URL? {
        let flat = text.replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !flat.isEmpty,
              let encoded = flat.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)
        else { return nil }
        return URL(string: "https://maps.apple.com/?q=\(encoded)")
    }
}
