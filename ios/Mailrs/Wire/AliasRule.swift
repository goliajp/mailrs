import Foundation

/// What makes an alias worth sending to the server.
///
/// The rules are the server's, restated where the person is typing so
/// a mistake costs a moment rather than a round trip: an alias routes
/// one address to another, both need to be addresses, and routing an
/// address to itself is a loop the server would take and the mail
/// would never leave.
enum AliasRule {
    static func domain(of address: String) -> String {
        let email = SenderName.extractEmail(address)
        guard let at = email.lastIndex(of: "@") else { return "" }
        return String(email[email.index(after: at)...])
    }

    /// Whether this pair can be created. Deliberately not an RFC 5322
    /// validator — the server does the real check, and a client-side
    /// grammar that rejects a valid address is worse than one that
    /// lets a typo through.
    static func isCreatable(source: String, target: String) -> Bool {
        let from = SenderName.extractEmail(source.trimmingCharacters(in: .whitespaces))
        let to = SenderName.extractEmail(target.trimmingCharacters(in: .whitespaces))
        guard looksLikeAddress(from), looksLikeAddress(to) else { return false }
        return from != to
    }

    private static func looksLikeAddress(_ address: String) -> Bool {
        guard let at = address.firstIndex(of: "@") else { return false }
        let local = address[address.startIndex..<at]
        let domain = address[address.index(after: at)...]
        return !local.isEmpty && domain.contains(".") && !domain.hasSuffix(".")
    }
}
