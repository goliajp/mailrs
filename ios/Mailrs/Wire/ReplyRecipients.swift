import Foundation

/// Who a reply goes to — the web's rules
/// (`thread-view.tsx` / `mobile-mail.tsx`), ported:
///
/// - Reply: the sender of the message being answered.
/// - Reply-all: the sender plus everyone on the To line, minus yourself
///   — a reply must not arrive addressed back at the person sending it.
/// - Subjects gain `Re:` / `Fwd:` unless already carrying one. The
///   prefix check is case-insensitive where the web's is not: "RE: x"
///   should not become "Re: RE: x", and answering with the superset
///   costs nothing.
enum ReplyRecipients {
    static func reply(toSender sender: String) -> [String] {
        [SenderName.extractEmail(sender)]
    }

    /// `recipients` is the wire's comma/semicolon-joined To line; the
    /// entries may be display forms. Order is sender first, then the To
    /// line in its own order; duplicates collapse to first appearance.
    static func replyAll(sender: String, recipients: String, myAddress: String) -> [String] {
        let mine = myAddress.lowercased()
        var seen = Set<String>()
        var out: [String] = []
        let all = [sender] + recipients.split(whereSeparator: { $0 == "," || $0 == ";" }).map(String.init)
        for entry in all {
            let email = SenderName.extractEmail(entry.trimmingCharacters(in: .whitespaces))
            if email.isEmpty || email == mine || seen.contains(email) { continue }
            seen.insert(email)
            out.append(email)
        }
        return out
    }

    static func subject(_ original: String, forwarding: Bool) -> String {
        let prefix = prefixFor(forwarding: forwarding)
        if original.lowercased().hasPrefix(prefix.lowercased()) { return original }
        if original.isEmpty { return prefix }
        return "\(prefix) \(original)"
    }

    private static func prefixFor(forwarding: Bool) -> String {
        if forwarding { return "Fwd:" }
        return "Re:"
    }
}
