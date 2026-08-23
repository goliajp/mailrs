import Foundation

/// RFC 2047 encoded words — `=?UTF-8?B?...?=`.
///
/// A header may only hold ASCII, so anything else arrives encoded.
/// Without this every Japanese or Chinese subject in the list is a run
/// of `=?UTF-8?B?` gibberish, which is the most visible way a mail
/// client can look broken.
enum EncodedWord {
    /// Decode every encoded word in a header value.
    ///
    /// Text outside the words is left exactly as it is: a subject is
    /// often half encoded and half not, and re-encoding the plain half
    /// would corrupt it.
    static func decode(_ value: String) -> String {
        guard value.contains("=?") else { return value }
        var out = ""
        var rest = Substring(value)
        /// Whether the previous chunk was an encoded word.
        ///
        /// RFC 2047 §6.2: whitespace **between two encoded words** is
        /// not part of the text — it is there so the words can be
        /// folded, and a decoder that keeps it puts a space in the
        /// middle of every long CJK subject.
        var previousWasWord = false

        while let start = rest.range(of: "=?") {
            let before = rest[..<start.lowerBound]
            guard let word = readWord(rest[start.lowerBound...]) else {
                out += before + "=?"
                rest = rest[start.upperBound...]
                previousWasWord = false
                continue
            }
            let gap = String(before)
            if !(previousWasWord && gap.allSatisfy(\.isWhitespace) && !gap.isEmpty) {
                out += gap
            }
            out += word.text
            rest = word.rest
            previousWasWord = true
        }
        out += rest
        return out
    }

    private struct Word {
        let text: String
        let rest: Substring
    }

    /// One `=?charset?enc?payload?=`, from a slice that starts at `=?`.
    ///
    /// Scanned by index rather than split on `?`: splitting consumes
    /// the `?` that `?=` is made of, so the closing delimiter is no
    /// longer there to find. That is not a subtle failure — it decodes
    /// nothing at all, and every encoded subject stays raw on screen.
    private static func readWord(_ s: Substring) -> Word? {
        let body = s.dropFirst(2)
        guard let firstQ = body.firstIndex(of: "?") else { return nil }
        let afterFirst = body.index(after: firstQ)
        guard let secondQ = body[afterFirst...].firstIndex(of: "?") else { return nil }
        let afterSecond = body.index(after: secondQ)
        guard let close = body[afterSecond...].range(of: "?=") else { return nil }

        let charset = String(body[..<firstQ]).uppercased()
        let encoding = String(body[afterFirst..<secondQ]).uppercased()
        let payload = String(body[afterSecond..<close.lowerBound])

        let bytes: Data?
        switch encoding {
        case "B": bytes = Data(base64Encoded: padded(payload))
        case "Q": bytes = quotedPrintable(payload)
        default: return nil
        }
        guard let bytes, let text = string(bytes, charset: charset) else { return nil }
        return Word(text: text, rest: body[close.upperBound...])
    }

    /// Base64 without its padding is common in the wild and decodes to
    /// nil without it.
    private static func padded(_ s: String) -> String {
        let short = s.count % 4
        return short == 0 ? s : s + String(repeating: "=", count: 4 - short)
    }

    /// Q-encoding: `_` is a space, `=XX` is a byte.
    private static func quotedPrintable(_ s: String) -> Data? {
        var out = Data()
        var i = s.startIndex
        while i < s.endIndex {
            let c = s[i]
            if c == "_" {
                out.append(0x20)
                i = s.index(after: i)
            } else if c == "=", s.distance(from: i, to: s.endIndex) >= 3 {
                let hex = s[s.index(after: i)..<s.index(i, offsetBy: 3)]
                guard let b = UInt8(hex, radix: 16) else { return nil }
                out.append(b)
                i = s.index(i, offsetBy: 3)
            } else {
                out.append(contentsOf: Array(String(c).utf8))
                i = s.index(after: i)
            }
        }
        return out
    }

    /// The charsets that actually turn up. An unknown one returns nil
    /// so the raw word is left visible — mojibake somebody can report
    /// beats text this app invented.
    private static func string(_ bytes: Data, charset: String) -> String? {
        switch charset {
        case "UTF-8", "UTF8": String(data: bytes, encoding: .utf8)
        case "ISO-8859-1", "LATIN1": String(data: bytes, encoding: .isoLatin1)
        case "ISO-2022-JP": String(data: bytes, encoding: .iso2022JP)
        case "SHIFT_JIS", "SHIFT-JIS", "SJIS": String(data: bytes, encoding: .shiftJIS)
        case "EUC-JP": String(data: bytes, encoding: .japaneseEUC)
        case "GB2312", "GBK", "GB18030":
            String(data: bytes, encoding: String.Encoding(rawValue:
                CFStringConvertEncodingToNSStringEncoding(
                    CFStringEncoding(CFStringEncodings.GB_18030_2000.rawValue))))
        default: nil
        }
    }
}
