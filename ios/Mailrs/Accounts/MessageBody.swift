import Foundation

/// The readable part of a message, out of its raw bytes.
///
/// Not a full MIME implementation and not trying to be: what a reader
/// needs is the one part worth showing and the text of it. Attachments,
/// signatures and the rest of the tree are left alone.
///
/// Bytes rather than a `String` throughout, and that is the whole
/// reason this exists separately from `MessageHeaders`. A message says
/// what its charset is *inside itself*; decoding it as UTF-8 on the way
/// in — which is what reading a socket into a `String` does — turns
/// every Shift_JIS and windows-1252 message into replacement
/// characters before anything has read the header that would have said
/// so.
enum MessageBody {
    struct Display: Equatable {
        var text: String
        /// Whether `text` is HTML. The caller renders accordingly; a
        /// reader shown raw markup is a reader shown a defect.
        var isHTML: Bool

        static let empty = Display(text: "", isHTML: false)
    }

    /// The part worth showing.
    static func extract(_ raw: Data) -> Display {
        let (headerBytes, body) = split(raw)
        let header = String(decoding: headerBytes, as: UTF8.self)
        return part(header: header, body: body)
    }

    // MARK: - structure

    private static func part(header: String, body: Data) -> Display {
        let type = contentType(header)
        if type.type == "multipart" {
            guard let boundary = type.params["boundary"], !boundary.isEmpty else {
                // A multipart with no boundary cannot be taken apart.
                // Showing the raw source beats showing nothing: the
                // text is usually in there, sandwiched between header
                // blocks a person can read past. Deliberately not via
                // `decoded`, which would throw it away for not being
                // `text/*` — it is not text/* by declaration only.
                return Display(
                    text: string(
                        decodeTransfer(body, as: encoding(header)),
                        charset: type.params["charset"]),
                    isHTML: false)
            }
            return choose(among: pieces(of: body, boundary: boundary), kind: type.subtype)
        }
        return decoded(header: header, body: body)
    }

    /// Which of a multipart's pieces to show.
    ///
    /// `alternative` is the same message written twice, so the choice
    /// is a preference: plain text first, markup only when there is no
    /// plain text. Every other kind — `mixed`, `related`, `signed` — is
    /// a message plus its attachments, and the first piece with
    /// anything readable in it is the message.
    private static func choose(among parts: [Data], kind: String) -> Display {
        let shown = parts.map { piece -> Display in
            let (h, b) = split(piece)
            return part(header: String(decoding: h, as: UTF8.self), body: b)
        }
        if kind == "alternative" {
            if let plain = shown.first(where: { !$0.isHTML && !$0.text.isEmpty }) { return plain }
        }
        return shown.first(where: { !$0.text.isEmpty }) ?? .empty
    }

    /// The pieces between the boundary delimiters.
    ///
    /// Everything before the first delimiter is the preamble and
    /// everything after the closing one is the epilogue; both are
    /// there for mail readers that cannot do MIME at all, and neither
    /// is part of the message.
    static func pieces(of body: Data, boundary: String) -> [Data] {
        let delimiter = Data("--\(boundary)".utf8)
        var out: [Data] = []
        var starts = ranges(of: delimiter, in: body)
        guard !starts.isEmpty else { return [] }
        // The delimiter must begin a line, or a boundary string that
        // happens to appear inside a part cuts it in half.
        //
        // `startIndex`, never 0: every `Data` here is a **slice** of
        // the message, whose indices run from where the slice began.
        // Comparing against 0 reads before the slice, and indexing by
        // `count` reads past its end — which is what crashed four of
        // these tests outright rather than failing them.
        starts = starts.filter { $0 == body.startIndex || body[$0 - 1] == 0x0A }
        for (i, start) in starts.enumerated() {
            let afterDelimiter = start + delimiter.count
            if afterDelimiter + 1 < body.endIndex,
                body[afterDelimiter] == 0x2D, body[afterDelimiter + 1] == 0x2D
            { break }  // the closing delimiter: nothing after it is ours
            guard i + 1 < starts.count else { break }
            let from = skipLine(body, afterDelimiter)
            let to = starts[i + 1]
            if from < to { out.append(body[from..<to]) }
        }
        return out
    }

    // MARK: - leaves

    private static func decoded(header: String, body: Data) -> Display {
        let type = contentType(header)
        // Anything that is not text is not something to show as text.
        // An attached PDF decoded as if it were a message reads as a
        // screen of noise.
        if !type.type.isEmpty && type.type != "text" { return .empty }
        let bytes = decodeTransfer(body, as: encoding(header))
        let text = string(bytes, charset: type.params["charset"])
        return Display(text: text, isHTML: type.subtype == "html")
    }

    static func decodeTransfer(_ body: Data, as cte: String) -> Data {
        switch cte {
        case "base64": return Base64Body.decode(body)
        case "quoted-printable": return QuotedPrintable.decode(body)
        default: return body
        }
    }

    /// Bytes to text, in the charset the message declared.
    ///
    /// UTF-8 when nothing was declared — it is what most mail is, and
    /// it is self-checking, so a wrong guess fails loudly enough to
    /// fall back rather than producing plausible nonsense.
    private static func string(_ bytes: Data, charset: String?) -> String {
        guard let charset, !charset.isEmpty else {
            return String(decoding: bytes, as: UTF8.self)
        }
        let cf = CFStringConvertIANACharSetNameToEncoding(charset as CFString)
        if cf != kCFStringEncodingInvalidId {
            let ns = CFStringConvertEncodingToNSStringEncoding(cf)
            if let text = String(data: bytes, encoding: String.Encoding(rawValue: ns)) {
                return text
            }
        }
        return String(decoding: bytes, as: UTF8.self)
    }

    // MARK: - headers of a part

    struct ContentType: Equatable {
        var type = ""
        var subtype = ""
        var params: [String: String] = [:]
    }

    static func contentType(_ header: String) -> ContentType {
        guard let raw = value(of: "content-type", in: header) else {
            return ContentType(type: "text", subtype: "plain")
        }
        var out = ContentType()
        let fields = splitOnSemicolons(raw)
        let full = fields.first ?? ""
        let halves = full.split(separator: "/", maxSplits: 1)
        out.type = halves.first.map { String($0).lowercased() } ?? ""
        if halves.count > 1 { out.subtype = String(halves[1]).lowercased() }
        for field in fields.dropFirst() {
            guard let eq = field.firstIndex(of: "=") else { continue }
            let name = field[..<eq].trimmingCharacters(in: .whitespaces).lowercased()
            var v = field[field.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            if v.hasPrefix("\"") && v.hasSuffix("\"") && v.count >= 2 { v = String(v.dropFirst().dropLast()) }
            out.params[name] = v
        }
        return out
    }

    static func encoding(_ header: String) -> String {
        value(of: "content-transfer-encoding", in: header)?
            .trimmingCharacters(in: .whitespaces).lowercased() ?? ""
    }

    private static func value(of name: String, in header: String) -> String? {
        for line in MessageHeaders.unfolded(header) {
            guard let colon = line.firstIndex(of: ":") else { continue }
            if line[..<colon].lowercased().trimmingCharacters(in: .whitespaces) == name {
                return String(line[line.index(after: colon)...])
                    .trimmingCharacters(in: .whitespaces)
            }
        }
        return nil
    }

    /// Split on the semicolons that separate parameters, ignoring the
    /// ones inside a quoted value — a boundary may contain one, and
    /// splitting there loses the rest of it.
    private static func splitOnSemicolons(_ s: String) -> [String] {
        var out: [String] = []
        var current = ""
        var quoted = false
        for ch in s {
            if ch == "\"" { quoted.toggle() }
            if ch == ";" && !quoted {
                out.append(current.trimmingCharacters(in: .whitespaces))
                current = ""
                continue
            }
            current.append(ch)
        }
        out.append(current.trimmingCharacters(in: .whitespaces))
        return out.filter { !$0.isEmpty }
    }

    // MARK: - bytes

    /// Header block and body, at the first blank line.
    static func split(_ raw: Data) -> (Data, Data) {
        if let r = ranges(of: Data("\r\n\r\n".utf8), in: raw).first {
            return (raw[raw.startIndex..<r], raw[(r + 4)...])
        }
        if let r = ranges(of: Data("\n\n".utf8), in: raw).first {
            return (raw[raw.startIndex..<r], raw[(r + 2)...])
        }
        return (raw, Data())
    }

    private static func skipLine(_ d: Data, _ from: Int) -> Int {
        var i = from
        while i < d.endIndex && d[i] != 0x0A { i += 1 }
        return min(i + 1, d.endIndex)
    }

    private static func ranges(of needle: Data, in haystack: Data) -> [Int] {
        guard !needle.isEmpty, haystack.count >= needle.count else { return [] }
        var out: [Int] = []
        let last = haystack.endIndex - needle.count
        var i = haystack.startIndex
        while i <= last {
            if haystack[i] == needle[needle.startIndex] {
                var match = true
                for k in 0..<needle.count where haystack[i + k] != needle[needle.startIndex + k] {
                    match = false
                    break
                }
                if match { out.append(i) }
            }
            i += 1
        }
        return out
    }
}
