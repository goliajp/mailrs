import Foundation

/// Phone numbers a message asks the reader to call, and which of them the
/// same sender has not written before in this conversation.
///
/// A number proves nothing about who answers it, so this only points it
/// out; it never judges the message. The rule and its limits are the web
/// client's, so the three clients point at the same numbers.
enum CallbackNumbers {
    private static let candidate = try? NSRegularExpression(
        pattern: #"(?:\+|\(\d)?\d[\d\s.()\-‐－]{7,22}\d"#)
    private static let minDigits = 10
    private static let maxDigits = 15
    private static let maxShown = 3

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
    static func phoneNumbers(in text: String) -> [String] {
        guard let candidate else { return [] }
        let ns = text as NSString
        var seen: Set<String> = []
        var out: [String] = []
        for match in candidate.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            let written = ns.substring(with: match.range).trimmingCharacters(in: .whitespaces)
            let d = digits(written)
            guard (minDigits...maxDigits).contains(d.count) else { continue }
            // an unbroken run is an order or account number, not a number to dial
            let grouped = written.hasPrefix("+")
                || written.contains(where: { " .()-‐－".contains($0) || $0.isWhitespace })
            // dates and times written with dots or dashes: 2026-11-01 12.30
            let dateLike = written.firstMatch(of: /^\d{4}[-.]\d{1,2}[-.]\d{1,2}/) != nil
            guard grouped, !dateLike, !seen.contains(d) else { continue }
            seen.insert(d)
            out.append(written)
        }
        return out
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
