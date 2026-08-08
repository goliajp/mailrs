import Foundation

/// Which of my addresses a message actually arrived at.
///
/// Mail to `sales@golia.jp` and mail to `lihao@golia.jp` land in the
/// same mailbox and, until now, looked identical once they got there —
/// so a message written to a role address, a signup address, or a
/// forwarding address gave no sign of it. That is exactly the fact that
/// decides how to answer: whether to reply as a person or as a desk,
/// and, for an address that only one service was ever given, whether
/// mail arriving at it can be genuine at all.
///
/// The rule is deliberately conservative. It marks only when the direct
/// address is **absent** — a message addressed to both is not "via" an
/// alias, it is addressed to me — and only when the alias points at me,
/// so someone else's alias in the recipient list stays someone else's.
enum AliasMark {
    /// The alias this message came to, if it came to one.
    ///
    /// `recipients` is the wire's comma/semicolon-joined To line, whose
    /// entries may be display forms (`Sales <sales@golia.jp>`).
    static func arrivedVia(
        recipients: String, myAddress: String, aliases: [Wire.Alias]
    ) -> String? {
        let mine = myAddress.lowercased()
        guard !mine.isEmpty else { return nil }
        let addressed = emails(in: recipients)
        guard !addressed.contains(mine) else { return nil }

        let toMe = aliases.filter { $0.targetAddress.lowercased() == mine }
        for address in addressed {
            if let exact = toMe.first(where: { $0.sourceAddress.lowercased() == address }) {
                return exact.sourceAddress
            }
        }
        // Catch-alls last: an exact alias is the better answer whenever
        // both could match, and on a domain that forwards everything the
        // exact one is the address someone actually chose to write to.
        for address in addressed {
            guard let domain = address.split(separator: "@").last.map(String.init) else { continue }
            if let catchAll = toMe.first(where: { isCatchAll($0.sourceAddress, for: domain) }) {
                // The address the sender used, not the `@domain` pattern
                // that routed it — "via @golia.jp" names a rule, and the
                // reader wants the address.
                _ = catchAll
                return address
            }
        }
        return nil
    }

    /// `@golia.jp` and `*@golia.jp` both mean "anything at this domain",
    /// which is how the catch-all aliases in this deployment are
    /// written.
    static func isCatchAll(_ source: String, for domain: String) -> Bool {
        let lowered = source.lowercased()
        let target = domain.lowercased()
        if lowered == "@\(target)" { return true }
        if lowered == "*@\(target)" { return true }
        return false
    }

    private static func emails(in recipients: String) -> [String] {
        recipients
            .split(whereSeparator: { $0 == "," || $0 == ";" })
            .map { SenderName.extractEmail($0.trimmingCharacters(in: .whitespaces)).lowercased() }
            .filter { !$0.isEmpty }
    }
}
