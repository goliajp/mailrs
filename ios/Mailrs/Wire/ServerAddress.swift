import Foundation

/// The address of a mail server, as typed and as kept.
///
/// The same rule as `ServerAddress` on Android: the two apps must agree
/// on which typed strings are one server, or a list made on one phone
/// reads differently on the other.
enum ServerAddress {
    /// `mail.example.com`, `https://mail.example.com/` and
    /// `HTTPS://Mail.Example.com` are one server; this is the form it is
    /// stored and compared in. Nil when what was typed cannot be one.
    static func normalise(_ text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.contains(where: \.isWhitespace) else { return nil }

        var scheme = "https"
        var rest = Substring(trimmed)
        if let mark = trimmed.range(of: "://") {
            scheme = trimmed[..<mark.lowerBound].lowercased()
            rest = trimmed[mark.upperBound...]
        }
        guard scheme == "http" || scheme == "https" else { return nil }
        // a query, a fragment or a user name is not part of where a server is
        guard !rest.contains(where: { "?#@".contains($0) }) else { return nil }

        let slash = rest.firstIndex(of: "/") ?? rest.endIndex
        var path = String(rest[slash...])
        while path.hasSuffix("/") { path.removeLast() }
        guard let authority = authority(String(rest[..<slash])) else { return nil }
        return scheme + "://" + authority + path
    }

    /// Host and optional port, lowercased; nil when either is malformed.
    private static func authority(_ text: String) -> String? {
        var host = text
        var port = ""
        if text.hasPrefix("[") {
            // an IPv6 literal carries colons of its own
            guard let close = text.firstIndex(of: "]") else { return nil }
            host = String(text[...close])
            port = String(text[text.index(after: close)...])
            guard port.isEmpty || port.hasPrefix(":") else { return nil }
            port = String(port.dropFirst())
            guard host.count > 2, host.dropFirst().dropLast().allSatisfy({ $0.isHexDigit || $0 == ":" || $0 == "." })
            else { return nil }
        } else {
            let pieces = text.split(separator: ":", omittingEmptySubsequences: false)
            guard pieces.count <= 2 else { return nil }
            host = String(pieces[0])
            if pieces.count == 2 {
                port = String(pieces[1])
                guard !port.isEmpty else { return nil }
            }
            guard looksLikeHost(host) else { return nil }
        }
        if port.isEmpty { return host.lowercased() }
        guard port.allSatisfy(\.isASCII), let number = Int(port), (1...65535).contains(number) else { return nil }
        return host.lowercased() + ":" + String(number)
    }

    private static func looksLikeHost(_ host: String) -> Bool {
        guard !host.isEmpty, !host.contains("..") else { return false }
        guard let first = host.first, let last = host.last else { return false }
        guard !".-".contains(first), !".-".contains(last) else { return false }
        return host.allSatisfy { $0.isLetter || $0.isNumber || "-._".contains($0) }
    }

    /// What a row shows for a stored address: the host alone when the
    /// connection is encrypted, the whole thing when it is not, so plain
    /// `http://` is never hidden behind a tidy name.
    static func display(_ normalised: String) -> String {
        if normalised.hasPrefix("https://") { return String(normalised.dropFirst("https://".count)) }
        return normalised
    }

    /// The host of a stored address, the name a server gets when none was typed.
    static func host(_ normalised: String) -> String {
        let rest = normalised.range(of: "://").map { normalised[$0.upperBound...] } ?? Substring(normalised)
        let authority = rest.prefix { $0 != "/" }
        if authority.hasPrefix("["), let close = authority.firstIndex(of: "]") {
            return String(authority[...close])
        }
        return String(authority.prefix { $0 != ":" })
    }
}
