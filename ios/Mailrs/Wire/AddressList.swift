import Foundation

/// Turning a typed "To" field into the array the server takes.
///
/// The same rule as the web client's `parseAddressList` in
/// `lib/send-mail.ts` — split on comma or semicolon, trim, drop the
/// empties — so the two clients cannot disagree about what "a@b.jp,
/// c@d.jp" means. Ported rather than reinvented; if this changes, change
/// that in the same commit.
enum AddressList {
    static func parse(_ input: String) -> [String] {
        input
            .split(whereSeparator: { $0 == "," || $0 == ";" })
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    /// Whether this is worth letting someone press Send on.
    ///
    /// Deliberately not an RFC 5322 validator: the address grammar
    /// permits far more than anyone types, the server does the real
    /// check, and a client-side regex that rejects a valid address is
    /// worse than one that lets a typo through. This asks only whether
    /// there is at least one entry that could be an address at all.
    static func isSendable(_ input: String) -> Bool {
        let addresses = parse(input)
        guard !addresses.isEmpty else { return false }
        return addresses.allSatisfy { address in
            guard let at = address.firstIndex(of: "@") else { return false }
            let local = address[address.startIndex..<at]
            let domain = address[address.index(after: at)...]
            return !local.isEmpty && domain.contains(".") && !domain.hasSuffix(".")
        }
    }
}
