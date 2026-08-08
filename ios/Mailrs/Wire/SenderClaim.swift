import Foundation

/// When a sender's display name claims one domain and the mail came
/// from another.
///
/// iOS offers an app **nothing** here: there is no fraudulent-content or
/// safe-browsing API in the SDK, and the protection people associate
/// with the platform lives inside Safari, reached only by handing a URL
/// over. Everything a mail client can say about a sender it has to work
/// out itself.
///
/// Two candidate signals were measured against this mailbox before
/// either was built, and only one survived:
///
/// - **Link text versus href.** 851 HTML messages, 13,113 anchors. 59
///   messages (7%) had an anchor whose text was a domain differing from
///   its target — and nearly every one was legitimate click tracking:
///   Mailchimp wrapping qiita.com, Substack wrapping amazon.com, SES
///   wrapping the sender's own site. A warning that fires on 7% of
///   ordinary mail teaches people to tap through it. Not built.
///
/// - **Display name versus sending domain.** 1,500 From headers. 206
///   display names contain a domain-like token; **8** disagree with the
///   domain that actually sent the mail. Six of those eight are
///   unambiguous impersonation — `amazon.co.jp` sent from
///   `mail07.jqjintaiyang.com`, and this deployment's own `golia.jp`
///   sent from `exportesram-ems.cam`. Two are the same company using a
///   second domain.
///
/// So this exists and the other does not. And it does not *accuse*: it
/// states where the message came from. A false positive then costs the
/// reader nothing — being told the real domain of a legitimate sender is
/// information, not an alarm — which is the property the link heuristic
/// could not have.
enum SenderClaim {
    /// A recognisable domain inside a display name.
    ///
    /// Deliberately a short, common list of suffixes rather than the
    /// public suffix list: the point is to catch a name a human would
    /// read *as* a domain, and `amazon.co.jp` in a display name is that
    /// whether or not a full PSL agrees about its boundaries.
    private static let suffixes = [
        "com", "net", "org", "jp", "ai", "io", "dev", "me", "cn", "co", "app", "shop",
    ]

    /// The domain the mail actually came from, when the display name
    /// says a different one. `nil` when the name claims nothing, or
    /// claims something the address agrees with.
    static func contradictedDomain(displayName: String, address: String) -> String? {
        guard let sending = domain(of: address) else { return nil }
        guard let claimed = claimedDomain(in: displayName) else { return nil }
        if agree(claimed: claimed, sending: sending) { return nil }
        return sending
    }

    /// The first domain-shaped token in a display name.
    static func claimedDomain(in displayName: String) -> String? {
        let lowered = displayName.lowercased()
        // Split on everything a label cannot contain, so "Amazon.co.jp
        // <no-reply>" and "【Amazon.co.jp】" both yield the token.
        let tokens = lowered.split(whereSeparator: { !($0.isLetter || $0.isNumber || $0 == "." || $0 == "-") })
        for token in tokens {
            let candidate = String(token).trimmingCharacters(in: CharacterSet(charactersIn: ".-"))
            let labels = candidate.split(separator: ".")
            guard labels.count >= 2, let last = labels.last else { continue }
            guard suffixes.contains(String(last)) else { continue }
            // A bare "co.jp" is a suffix, not a claim; and a label has
            // to have something in it.
            guard labels.count >= 2, labels.dropLast().allSatisfy({ !$0.isEmpty }) else { continue }
            if labels.count == 2, suffixes.contains(String(labels[0])) { continue }
            return candidate
        }
        return nil
    }

    /// Related enough that saying so would be noise.
    ///
    /// A subdomain of the claim, the claim being a subdomain of the
    /// sender, or one containing the other's registrable part — mail
    /// from `email.amazon.co.jp` for a name saying `amazon.co.jp` is the
    /// ordinary case and must stay silent.
    static func agree(claimed: String, sending: String) -> Bool {
        if claimed == sending { return true }
        if sending.hasSuffix("." + claimed) { return true }
        if claimed.hasSuffix("." + sending) { return true }
        return false
    }

    static func domain(of address: String) -> String? {
        let email = SenderName.extractEmail(address).lowercased()
        guard let at = email.lastIndex(of: "@") else { return nil }
        let domain = String(email[email.index(after: at)...])
        guard domain.contains("."), !domain.isEmpty else { return nil }
        return domain
    }
}
