import Foundation

/// Turning a From header into the name a row shows.
///
/// Ported from `web/src/lib/avatar.ts`, rule for rule, and mirrored by
/// the same test cases — the two clients must put the same face on the
/// same mail. The pipeline: RFC 2047 encoded-words decoded, then
/// `"Name" <addr>` yields the name, a bare address yields its local
/// part, and machine-generated local parts (bounce chains, VERP, hashes)
/// yield a capitalized brand from the domain instead — `notify@
/// em8742.bsm.freee.work` reads "Freee", not a hash.
enum SenderName {
    /// RFC 2047 encoded-words (`=?UTF-8?B?…?=` / `?Q?…?=`) in headers.
    static func decodeMimeHeader(_ value: String) -> String {
        guard value.contains("=?") else { return value }
        var result = value
        let pattern = /=\?([^?]+)\?([BbQq])\?([^?]*)\?=/
        while let match = result.firstMatch(of: pattern) {
            let charset = String(match.1).lowercased()
            let encoding = String(match.2).uppercased()
            let payload = String(match.3)
            let decoded: String
            if encoding == "B" {
                decoded = Data(base64Encoded: payload)
                    .flatMap { decode($0, charset: charset) } ?? payload
            } else {
                var bytes: [UInt8] = []
                var index = payload.startIndex
                while index < payload.endIndex {
                    let ch = payload[index]
                    if ch == "_" {
                        bytes.append(0x20)
                        index = payload.index(after: index)
                    } else if ch == "=",
                              let end = payload.index(index, offsetBy: 3, limitedBy: payload.endIndex),
                              let byte = UInt8(payload[payload.index(after: index)..<end], radix: 16) {
                        bytes.append(byte)
                        index = end
                    } else {
                        bytes.append(contentsOf: Array(String(ch).utf8))
                        index = payload.index(after: index)
                    }
                }
                decoded = decode(Data(bytes), charset: charset) ?? payload
            }
            result.replaceSubrange(match.range, with: decoded)
        }
        return result
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
    }

    static func extractEmail(_ sender: String) -> String {
        let decoded = decodeMimeHeader(sender)
        if let match = decoded.firstMatch(of: /<([^>]+)>/) {
            return String(match.1).lowercased()
        }
        return decoded.lowercased()
    }

    static func extractName(_ sender: String) -> String {
        let decoded = decodeMimeHeader(sender)
        if let match = decoded.firstMatch(of: /^"?([^"<]+)"?\s*</) {
            let name = String(match.1).trimmingCharacters(in: .whitespaces)
            if !name.contains("@"), !isMachineGenerated(name) { return name }
        }
        let email = extractEmail(decoded)
        let parts = email.split(separator: "@", maxSplits: 1)
        guard parts.count == 2 else { return email.isEmpty ? sender : email }
        let local = String(parts[0])
        let domain = String(parts[1])
        if isMachineGenerated(local) {
            let label = domainLabel(domain)
            return label.prefix(1).uppercased() + label.dropFirst()
        }
        return local
    }

    /// Tracking ids, bounce chains, VERP addresses, hashes.
    static func isMachineGenerated(_ s: String) -> Bool {
        if s.count <= 10 { return false }
        let lowered = s.lowercased()
        if lowered.hasPrefix("bounce") || lowered.hasPrefix("msprvs") || lowered.hasPrefix("prvs") {
            return true
        }
        if s.count > 15, s.contains("=") { return true }
        let digits = s.filter(\.isNumber).count
        if s.count > 12, Double(digits) / Double(s.count) > 0.3 { return true }
        if s.count > 20, !s.contains(" "), digits > 0 { return true }
        let letters = s.filter { $0.isLetter && $0.isASCII }.count
        if s.count > 15, Double(letters) / Double(s.count) < 0.5 { return true }
        return false
    }

    /// "notify.cloudflare.com" → "cloudflare"; "em8742.bsm.freee.work" →
    /// "freee" is what the machine-generated fallback shows.
    private static let secondaryTlds: Set<Substring> = [
        "ac", "co", "com", "edu", "gov", "ne", "net", "or", "org",
    ]

    static func domainLabel(_ domain: String) -> String {
        let parts = domain.split(separator: ".")
        guard parts.count > 1 else { return domain }
        let sld = parts[parts.count - 2]
        if parts.count >= 3, secondaryTlds.contains(sld) {
            return String(parts[parts.count - 3])
        }
        return String(sld)
    }

    /// The name a conversation row wears — the web's 2026-07-17 rule:
    /// the row's face is the OTHER participant. After you reply, your
    /// own address can bubble to `participants[0]`, and a row wearing
    /// your name reads like a sent mail sitting in the Inbox. Only a
    /// self-only thread says "Me".
    static func rowFace(participants: [String], myAddress: String) -> String {
        let mine = myAddress.lowercased()
        let others = participants.filter { extractEmail($0) != mine }
        if let other = others.first { return extractName(other) }
        if participants.isEmpty { return "(unknown)" }
        return "Me"
    }

    private static func decode(_ data: Data, charset: String) -> String? {
        switch charset {
        case "utf-8", "utf8": return String(data: data, encoding: .utf8)
        case "iso-2022-jp":
            return String(data: data, encoding: .iso2022JP)
        case "shift_jis", "shift-jis", "sjis":
            return String(data: data, encoding: .shiftJIS)
        case "iso-8859-1", "latin1":
            return String(data: data, encoding: .isoLatin1)
        default: return String(data: data, encoding: .utf8)
        }
    }
}
