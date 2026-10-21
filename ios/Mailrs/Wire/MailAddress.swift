import Foundation

/// The shape of a mailbox address, checked where it is typed.
///
/// A shape, not a verdict: the server decides whether the mailbox exists.
/// The same rule as `looksLikeAddress` on Android.
enum MailAddress {
    /// One `@`, something before it, and a domain with a dot that is not
    /// its first or last character; no whitespace anywhere.
    static func looksLike(_ text: String) -> Bool {
        guard !text.contains(where: \.isWhitespace) else { return false }
        let parts = text.split(separator: "@", omittingEmptySubsequences: false)
        guard parts.count == 2 else { return false }
        let local = parts[0]
        let domain = parts[1]
        return !local.isEmpty && domain.contains(".")
            && !domain.hasPrefix(".") && !domain.hasSuffix(".")
    }
}
