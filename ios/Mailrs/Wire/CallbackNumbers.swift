import Foundation

/// Phone numbers a message asks the reader to call, and which of them the
/// same sender has not written before in this conversation.
///
/// A number proves nothing about who answers it, so this only points it
/// out; it never judges the message. The rule and its limits are the web
/// client's, so the three clients point at the same numbers: the labels
/// come from `callback-numbers.json`, the web's table shipped in the app,
/// and the samples in the same file are what all three are tested against.
enum CallbackNumbers {
    /// One phone number, by its structure: an optional `+` and country
    /// code, an optional bracketed area code, then digit groups of at most
    /// six joined by one space, dot or hyphen each. A digit, letter or
    /// hyphen on either side means the digits belong to something else
    /// (2026-INV-003001-2), as do a currency sign, a thousands comma or a
    /// decimal point before them. A line break is not a separator.
    private static let candidate = try? NSRegularExpression(
        pattern: #"(?<![A-Za-z0-9_+.,/#\x24€£¥\-‐－])"#
            + #"(?:\+\d{1,3}[  .\-‐－]?)?(?:\(\d{1,4}\)[  .\-‐－]?)?"#
            + #"\d{1,6}(?:[  .\-‐－]\d{1,6}){1,5}"#
            + #"(?![A-Za-z0-9_:\-‐－]|[.,/]\d)"#)
    /// Dates and times written with dots, dashes or spaces: 2026-11-01 12.30
    private static let dateLike = try? NSRegularExpression(
        pattern: #"^\d{4}[-./ ‐－]\d{1,2}[-./ ‐－]\d{1,2}(?!\d)"#)
    /// 1 234 567 890: an amount grouped by thousands, not a number to dial
    private static let thousands = try? NSRegularExpression(
        pattern: #"^[1-9]\d{0,2}(?:[  .]\d{3})+$"#)
    /// Digits straight after one of these name an order, invoice or account.
    private static let afterLabel = try? NSRegularExpression(
        pattern: #"(?:^|[^A-Za-z0-9])(?:"#
            + labels.sorted { $0.utf16.count > $1.utf16.count }
                .map(NSRegularExpression.escapedPattern(for:))
                .joined(separator: "|")
            + #")[\s:：.#＃№\-]*$"#,
        options: .caseInsensitive)
    private static let labelWindow = 40
    private static let minDigits = 10
    /// A national number with no country code is at most 11 digits in the
    /// numbering plans this has to read (JP, US, UK).
    private static let maxNationalDigits = 11
    private static let maxDigits = 15
    private static let maxShown = 3

    private struct Table: Decodable {
        let labels: [String]
    }

    /// The labels in the table the app ships.
    static let labels: [String] = {
        let url = Bundle.main.url(forResource: "callback-numbers", withExtension: "json")!
        // the file ships inside the app, and the unit tests read this same copy
        return try! JSONDecoder().decode(Table.self, from: Data(contentsOf: url)).labels
    }()

    /// Numbers in `message` that no earlier message from the same address
    /// in `earlier` carried: a number a correspondent always signs with is
    /// not worth pointing out, a new one asking to be called is.
    static func new(in message: Wire.Message, earlier: [Wire.Message]) -> [String] {
        let own = message.sender.lowercased()
        let before = Set(
            earlier
                .filter { $0.sender.lowercased() == own }
                .flatMap { phoneNumbers(in: plainText($0)).map(digits) })
        return Array(
            phoneNumbers(in: plainText(message))
                .filter { !before.contains(digits($0)) }
                .prefix(maxShown))
    }

    /// Numbers in `text` that read as phone numbers, as written, once each.
    /// Misses some real ones on purpose: the notice is worth reading only
    /// while it is almost never wrong.
    static func phoneNumbers(in text: String) -> [String] {
        guard let candidate else { return [] }
        let ns = text as NSString
        var seen: Set<String> = []
        var out: [String] = []
        for match in candidate.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            let written = ns.substring(with: match.range)
            let d = digits(written)
            guard !seen.contains(d), dialable(written, d) else { continue }
            let from = max(0, match.range.location - labelWindow)
            let before = ns.substring(with: NSRange(location: from, length: match.range.location - from))
            guard !matches(afterLabel, before) else { continue }
            seen.insert(d)
            out.append(written)
        }
        return out
    }

    private static func dialable(_ written: String, _ d: String) -> Bool {
        guard d.count >= minDigits, d.count <= maxDigits(written) else { return false }
        return !matches(dateLike, written) && !matches(thousands, written)
    }

    private static func maxDigits(_ written: String) -> Int {
        if written.hasPrefix("+") { return maxDigits }
        return maxNationalDigits
    }

    private static func matches(_ regex: NSRegularExpression?, _ s: String) -> Bool {
        guard let regex else { return false }
        return regex.firstMatch(in: s, range: NSRange(location: 0, length: (s as NSString).length)) != nil
    }

    private static func digits(_ s: String) -> String {
        s.filter { $0.isASCII && $0.isNumber }
    }

    private static func plainText(_ m: Wire.Message) -> String {
        if let text = m.textBody, !text.isEmpty { return text }
        guard let html = m.htmlBody, !html.isEmpty else { return "" }
        return HTMLText.plain(html)
    }
}
