import Foundation

/// Reading what an IMAP server says.
///
/// Split from the socket so it can be tested without one: every
/// mistake worth making here is in the parsing, and a test that needs
/// a server is a test nobody runs.
enum IMAP {
    /// One untagged line, as far as this client cares about it.
    enum Untagged: Equatable {
        /// `* LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail"`
        case list(name: String, attributes: [String])
        /// `* 42 EXISTS`
        case exists(Int)
        /// `* OK [UIDVALIDITY 1234] ...`
        case uidValidity(UInt32)
        /// `* OK [UIDNEXT 4391] ...`
        case uidNext(UInt32)
        /// What the server says it can do.
        ///
        /// Announced in two places — the greeting and a `CAPABILITY`
        /// reply — and a client that reads only the second asks a
        /// question it already has the answer to.
        case capabilities([String])
        /// Anything this client has no use for.
        case other
    }

    /// How a tagged line ended.
    enum Completion: Equatable {
        case ok(String)
        case no(String)
        case bad(String)
    }

    /// The tagged reply for `tag`, or nil if this is not one.
    ///
    /// A tag is compared with a trailing space: `a1` must not match
    /// `a10`, and a server is free to interleave `a10`'s reply while
    /// `a1` is outstanding.
    static func completion(of line: String, tag: String) -> Completion? {
        guard line.hasPrefix(tag + " ") else { return nil }
        let rest = String(line.dropFirst(tag.count + 1))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let word = rest.split(separator: " ", maxSplits: 1).first.map(String.init) ?? ""
        let detail = rest.dropFirst(word.count).trimmingCharacters(in: .whitespaces)
        switch word.uppercased() {
        case "OK": return .ok(detail)
        case "NO": return .no(detail)
        case "BAD": return .bad(detail)
        default: return nil
        }
    }

    /// What an untagged line says, as far as this client uses it.
    static func untagged(_ line: String) -> Untagged? {
        guard line.hasPrefix("* ") else { return nil }
        let body = String(line.dropFirst(2)).trimmingCharacters(in: .whitespacesAndNewlines)

        if body.uppercased().hasPrefix("LIST ") {
            return parseList(body)
        }
        if body.uppercased().hasPrefix("CAPABILITY") {
            return .capabilities(body.split(separator: " ").dropFirst().map(String.init))
        }
        // Also announced inside the greeting's response code, which is
        // where a server that offers no `CAPABILITY` at all says it.
        if let open = body.range(of: "[CAPABILITY ", options: .caseInsensitive),
            let close = body[open.upperBound...].firstIndex(of: "]")
        {
            return .capabilities(
                body[open.upperBound..<close].split(separator: " ").map(String.init))
        }
        // `* 42 EXISTS` — a count, then the word.
        let parts = body.split(separator: " ", maxSplits: 1)
        if parts.count == 2, let n = Int(parts[0]),
           parts[1].uppercased().hasPrefix("EXISTS") {
            return .exists(n)
        }
        if let v = bracketed(body, "UIDVALIDITY") { return .uidValidity(v) }
        if let v = bracketed(body, "UIDNEXT") { return .uidNext(v) }
        return .other
    }

    /// `OK [UIDVALIDITY 1234] Ready` → 1234.
    ///
    /// The value is inside square brackets in a response code, and the
    /// text after it is free-form: a server may say anything at all,
    /// including something that looks like another number.
    private static func bracketed(_ body: String, _ key: String) -> UInt32? {
        guard let open = body.range(of: "[\(key) ", options: .caseInsensitive),
              let close = body.range(of: "]", range: open.upperBound..<body.endIndex)
        else { return nil }
        return UInt32(body[open.upperBound..<close.lowerBound]
            .trimmingCharacters(in: .whitespaces))
    }

    /// `LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail"`
    ///
    /// The name is last and may be quoted, may contain spaces, and may
    /// contain the delimiter — which is why it is taken from the end
    /// rather than by splitting on spaces.
    private static func parseList(_ body: String) -> Untagged? {
        guard let open = body.firstIndex(of: "("),
              let close = body.firstIndex(of: ")"), open < close
        else { return nil }
        let attributes = body[body.index(after: open)..<close]
            .split(separator: " ")
            .map(String.init)
        let tail = body[body.index(after: close)...]
            .trimmingCharacters(in: .whitespaces)
        // delimiter then name; the delimiter is quoted or NIL
        guard let name = lastQuotedOrWord(tail) else { return nil }
        return .list(name: name, attributes: attributes)
    }

    /// The last field of a line, unquoted.
    ///
    /// `"/" "[Gmail]/Sent Mail"` → `[Gmail]/Sent Mail`. Taken from the
    /// end because a mailbox name may hold spaces and the delimiter.
    static func lastQuotedOrWord(_ s: String) -> String? {
        let t = s.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty else { return nil }
        if t.hasSuffix("\"") {
            // Walk **forwards**, tracking escapes, and remember where
            // the last unescaped quote opened. Walking backwards and
            // stopping at the first unescaped quote finds the closing
            // one of an empty pair when the name itself ends in `\"`.
            let chars = Array(t)
            var opens: [Int] = []
            var i = 0
            while i < chars.count {
                if chars[i] == "\\" {
                    i += 2
                    continue
                }
                if chars[i] == "\"" { opens.append(i) }
                i += 1
            }
            guard opens.count >= 2 else { return nil }
            let open = opens[opens.count - 2]
            let close = opens[opens.count - 1]
            return String(chars[(open + 1)..<close])
                .replacingOccurrences(of: "\\\"", with: "\"")
                .replacingOccurrences(of: "\\\\", with: "\\")
        }
        return t.split(separator: " ").last.map(String.init)
    }

    /// What a `FETCH` line announced, when it announced a literal.
    ///
    /// `* 12 FETCH (UID 4390 FLAGS (\Seen) BODY[] {2048}` — the
    /// braces give the byte count, and **the bytes that follow are
    /// read by that count rather than scanned for a terminator**. A
    /// message body contains every byte sequence a terminator could be
    /// made of, so scanning truncates mail at whatever looks like the
    /// end.
    struct Announced: Equatable {
        let uid: UInt32?
        let seen: Bool
        /// How many bytes follow, when the line ends in a literal.
        let literalBytes: Int?
    }

    /// Read a `FETCH` line.
    static func fetchLine(_ line: String) -> Announced? {
        guard line.hasPrefix("* "), line.uppercased().contains(" FETCH ") else { return nil }
        let uid = number(after: "UID ", in: line).map { UInt32($0) }
        // `\Seen` inside the FLAGS list. Matched with the backslash so
        // a folder called "Seen" in the same line cannot set it.
        let seen = line.uppercased().contains("\\SEEN")
        var literal: Int?
        if let open = line.lastIndex(of: "{"), let close = line.lastIndex(of: "}"),
           open < close {
            literal = Int(line[line.index(after: open)..<close])
        }
        return Announced(uid: uid, seen: seen, literalBytes: literal)
    }

    /// The number after a keyword, or nil.
    private static func number(after keyword: String, in line: String) -> Int? {
        guard let r = line.range(of: keyword, options: .caseInsensitive) else { return nil }
        let digits = line[r.upperBound...].prefix { $0.isNumber }
        return digits.isEmpty ? nil : Int(digits)
    }

    /// Quote a mailbox name or a password for the wire.
    ///
    /// Generated app passwords contain `"` and `\` often enough that an
    /// unquoted argument turns one into a syntax error — and the person
    /// is told their password is wrong when it is right.
    static func quoted(_ s: String) -> String {
        let escaped = s
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        return "\"\(escaped)\""
    }

    /// Whether a refusal means the credential is wrong rather than the
    /// server being unhappy about something else.
    ///
    /// Worth telling apart: one is a button to press, the other is
    /// waiting. RFC 5530 gives the code; older servers only say it in
    /// the text.
    static func isAuthenticationFailure(_ detail: String) -> Bool {
        let d = detail.uppercased()
        return d.contains("[AUTHENTICATIONFAILED]")
            || d.contains("AUTHENTICATIONFAILED")
            || d.contains("INVALID CREDENTIALS")
            || d.contains("LOGIN FAILED")
            || d.contains("[AUTHORIZATIONFAILED]")
    }
}
