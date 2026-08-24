import Foundation

/// What is attached to a message.
///
/// A different question from "what should be shown", which is
/// `MessageBody`'s, and the two genuinely disagree: the part a reader
/// sees is not attached, and a PDF nobody can render still has to be
/// listed. Both walk the same tree through the same primitives, and
/// each applies its own policy to it.
enum MessageAttachments {
    /// One attached file, **not yet decoded**.
    ///
    /// It points into the message rather than carrying a copy: opening
    /// a 25 MB message used to hold the raw bytes and about 18 MB of
    /// decoded attachments at the same time, for a screen that shows a
    /// name and a size. The decoding happens when somebody taps one,
    /// and what is decoded is that one.
    ///
    /// `size` is computed from the encoded length rather than by
    /// decoding — base64 is four characters for every three bytes, so
    /// the answer is arithmetic, and a size shown before a tap must not
    /// cost what the tap costs.
    struct Attachment: Equatable, Identifiable {
        var filename: String
        var mimeType: String
        /// The encoded part, as it sits in the message.
        fileprivate var encoded: Data
        fileprivate var transfer: String
        /// Whether the message meant it to appear inside the text — a
        /// signature image, usually. Listed anyway, because a reader
        /// shown text has no other way to reach it, but marked so the
        /// list can say which is which.
        var inline: Bool

        /// How big the file is once decoded, without decoding it.
        ///
        /// base64 carries three bytes in every four characters, and
        /// the padding says how many of the last three are real. Line
        /// breaks are not characters of the encoding, so they are not
        /// counted.
        var size: Int {
            guard transfer == "base64" else { return encoded.count }
            var characters = 0
            var padding = 0
            for byte in encoded {
                switch byte {
                case UInt8(ascii: "="): padding += 1
                case UInt8(ascii: "A")...UInt8(ascii: "Z"),
                    UInt8(ascii: "a")...UInt8(ascii: "z"),
                    UInt8(ascii: "0")...UInt8(ascii: "9"),
                    UInt8(ascii: "+"), UInt8(ascii: "/"):
                    characters += 1
                default: break
                }
            }
            return max(0, (characters + padding) / 4 * 3 - padding)
        }

        /// The file itself, decoded now.
        func decoded() -> Data { MessageBody.decodeTransfer(encoded, as: transfer) }

        /// Unique within one message: two files may share a name, and
        /// a list keyed on the name alone shows one of them twice.
        var id: String { "\(filename)-\(size)-\(inline)" }
    }

    /// Everything attached, in the order the message lists it.
    static func of(_ raw: Data) -> [Attachment] {
        var out: [Attachment] = []
        walk(raw, into: &out)
        return out
    }

    private static func walk(_ raw: Data, into out: inout [Attachment]) {
        let (headerBytes, body) = MessageBody.split(raw)
        let header = String(decoding: headerBytes, as: UTF8.self)
        let type = MessageBody.contentType(header)
        if type.type == "multipart" {
            guard let boundary = type.params["boundary"], !boundary.isEmpty else { return }
            for piece in MessageBody.pieces(of: body, boundary: boundary) {
                walk(piece, into: &out)
            }
            return
        }
        let disposition = value("content-disposition", in: header) ?? ""
        let kind = disposition.prefix(while: { $0 != ";" })
            .trimmingCharacters(in: .whitespaces).lowercased()
        let name = filename(header)

        // A part is attached when the message says so, or when it is
        // not something a reader could have been shown. A text part
        // with no filename is the message itself.
        let attached = kind == "attachment" || name != nil || type.type != "text"
        guard attached else { return }
        out.append(
            Attachment(
                filename: name ?? fallbackName(type),
                mimeType: mimeType(type),
                encoded: body,
                transfer: MessageBody.encoding(header),
                inline: kind == "inline"))
    }

    private static func mimeType(_ type: MessageBody.ContentType) -> String {
        if type.type.isEmpty { return "application/octet-stream" }
        if type.subtype.isEmpty { return type.type }
        return "\(type.type)/\(type.subtype)"
    }

    /// The name, from wherever it was put.
    ///
    /// `Content-Disposition: attachment; filename=` is the right
    /// place; `Content-Type: ...; name=` is the older one and still
    /// arrives. Both may be RFC 2231-encoded, which is how a Japanese
    /// filename survives a header that must be ASCII — and a client
    /// that does not decode it shows the person
    /// `%E6%97%A5%E6%9C%AC.pdf`.
    static func filename(_ header: String) -> String? {
        let disposition = value("content-disposition", in: header) ?? ""
        let type = value("content-type", in: header) ?? ""
        for source in [disposition, type] {
            if let found = rfc2231(source, "filename") { return found }
            if let found = rfc2231(source, "name") { return found }
        }
        return nil
    }

    /// `filename="x"`, `filename*=utf-8\'\'%E2%80%A6`, and the numbered
    /// continuations a long name is split into.
    static func rfc2231(_ source: String, _ key: String) -> String? {
        let fields = source.split(separator: ";").map {
            $0.trimmingCharacters(in: .whitespaces)
        }
        // The continuations first: a name split across `key*0*=` and
        // `key*1*=` is not found by looking for `key*=` at all.
        var parts: [(Int, Bool, String)] = []
        for field in fields {
            guard let eq = field.firstIndex(of: "=") else { continue }
            let name = field[..<eq].trimmingCharacters(in: .whitespaces).lowercased()
            guard name.hasPrefix(key + "*") else { continue }
            var rest = Substring(name.dropFirst(key.count + 1))
            var encoded = false
            if rest.hasSuffix("*") {
                encoded = true
                rest = rest.dropLast()
            }
            guard let index = Int(rest) else { continue }
            parts.append((index, encoded, unquote(String(field[field.index(after: eq)...]))))
        }
        if !parts.isEmpty {
            let joined = parts.sorted { $0.0 < $1.0 }.map { _, encoded, value -> String in
                if encoded { return percentDecoded(stripCharset(value)) }
                return value
            }.joined()
            return joined.isEmpty ? nil : joined
        }
        for field in fields {
            guard let eq = field.firstIndex(of: "=") else { continue }
            let name = field[..<eq].trimmingCharacters(in: .whitespaces).lowercased()
            let value = unquote(String(field[field.index(after: eq)...]))
            if name == key + "*" { return percentDecoded(stripCharset(value)) }
            if name == key { return value }
        }
        return nil
    }

    /// `utf-8\'\'name` -> `name`, keeping the percent-escapes.
    private static func stripCharset(_ value: String) -> String {
        let quotes = value.split(separator: "\'", maxSplits: 2, omittingEmptySubsequences: false)
        guard quotes.count == 3 else { return value }
        return String(quotes[2])
    }

    /// Percent-escapes back to bytes, then to text as UTF-8.
    private static func percentDecoded(_ value: String) -> String {
        var bytes: [UInt8] = []
        var index = value.startIndex
        while index < value.endIndex {
            if value[index] == "%", let hexEnd = value.index(index, offsetBy: 3, limitedBy: value.endIndex) {
                let hex = value[value.index(after: index)..<hexEnd]
                if let byte = UInt8(hex, radix: 16) {
                    bytes.append(byte)
                    index = hexEnd
                    continue
                }
            }
            bytes.append(contentsOf: Array(String(value[index]).utf8))
            index = value.index(after: index)
        }
        return String(decoding: bytes, as: UTF8.self)
    }

    private static func unquote(_ value: String) -> String {
        let t = value.trimmingCharacters(in: .whitespaces)
        if t.count >= 2, t.hasPrefix("\""), t.hasSuffix("\"") {
            return String(t.dropFirst().dropLast())
        }
        return t
    }

    private static func value(_ name: String, in header: String) -> String? {
        for line in MessageHeaders.unfolded(header) {
            guard let colon = line.firstIndex(of: ":") else { continue }
            if line[..<colon].trimmingCharacters(in: .whitespaces).lowercased() == name {
                return String(line[line.index(after: colon)...])
                    .trimmingCharacters(in: .whitespaces)
            }
        }
        return nil
    }

    /// Something to call a nameless part.
    ///
    /// Not "attachment": a list of four things all called that is a
    /// list nobody can pick from. The type is what is actually known.
    private static func fallbackName(_ type: MessageBody.ContentType) -> String {
        var extension_ = type.subtype
        if type.subtype == "jpeg" { extension_ = "jpg" }
        if type.subtype == "plain" { extension_ = "txt" }
        if extension_.isEmpty { extension_ = "bin" }
        var base = type.type
        if base.isEmpty { base = "file" }
        return "\(base).\(extension_)"
    }
}
