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
    /// An encoded word decodes to **anything at all**, including a
    /// CRLF — and a header value cannot contain one. Folding is
    /// expressed by the encoding, never by the content, so a line
    /// break coming out of here did not come from a header: it came
    /// from somebody who wanted one somewhere it does not belong.
    ///
    /// Left in, it reached `RCPT TO:<…>` when replying (SMTP command
    /// injection: the message also went to an address the sender never
    /// typed) and the outgoing `To:` and `Subject:` (a `Bcc:` header
    /// the sender never wrote). Stripped here, at the boundary, rather
    /// than at each of the places that would have to remember.
    ///
    /// By scalar, because a CRLF is one `Character` in Swift and
    /// filtering Characters against `"\r"` matches neither half —
    /// the same trap that let a filename inject a header.
    private static func withoutControls(_ text: String) -> String {
        String(
            String.UnicodeScalarView(
                text.unicodeScalars.filter { $0.value >= 0x20 && $0.value != 0x7F }))
    }

    static func decode(_ value: String) -> String {
        guard value.contains("=?") else { return withoutControls(value) }
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
    /// The other direction: a header value that a receiving client
    /// can read.
    ///
    /// **ASCII passes through untouched.** Encoding a plain subject
    /// makes it unreadable in the raw message and gains nothing —
    /// which is why this checks rather than always encoding.
    ///
    /// Base64 rather than quoted-printable, because a subject that
    /// needs encoding at all is usually not Latin: a CJK subject in
    /// quoted-printable is three `=XX` per character and grows past
    /// the line limit almost at once.
    static func encode(_ text: String) -> String {
        guard text.contains(where: { !$0.isASCII }) else { return text }
        // 75 is the RFC 2047 limit for a whole encoded word including
        // its `=?utf-8?B?` and `?=`, and base64 is 4 characters per 3
        // bytes, so each chunk may carry 45 bytes at most.
        let chunks = utf8Chunks(text, bytes: 45)
        return chunks.map { "=?utf-8?B?\($0.base64EncodedString())?=" }
            .joined(separator: "\r\n ")
    }

    /// Split into runs of at most `bytes`, **never through a
    /// character**. Cutting UTF-8 mid-sequence produces an encoded word
    /// that decodes to a replacement character on every client.
    private static func utf8Chunks(_ text: String, bytes limit: Int) -> [Data] {
        var out: [Data] = []
        var current = Data()
        for character in text {
            let encoded = Data(String(character).utf8)
            if current.count + encoded.count > limit, !current.isEmpty {
                out.append(current)
                current = Data()
            }
            current.append(encoded)
        }
        if !current.isEmpty { out.append(current) }
        return out
    }

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
