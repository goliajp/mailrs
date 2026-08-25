import SwiftUI

/// Brand icons for sender domains, the web client's cascade.
///
/// Backend: `crates/webapi/src/handlers/icon.rs` —
/// `GET /api/icon/{domain}` walks BIMI, then the vendor-neutral
/// favicon services, and caches the answer in kevy for every user. It
/// answers **204, not 404**, when a domain has no icon anywhere; the
/// caller draws the coloured letter instead. (The web chose 204 so a
/// devtools console would not fill with red rows; the reason it
/// matters here is that a 404 reads as an error worth retrying and
/// this is not one.)
///
/// One in-flight request per domain, and the answer — icon *or*
/// nothing — is remembered for the app's lifetime: an inbox of fifty
/// rows from a dozen domains must not become fifty requests, and a
/// domain that has no icon must not be asked again on every scroll.
@MainActor
@Observable
final class SenderIcons {
    private var resolved: [String: Image?] = [:]
    private var inflight: Set<String> = []

    /// The icon if it is known, `nil` while unknown or absent. Asking
    /// starts the fetch; the property updates when it lands.
    func icon(for sender: String) -> Image? {
        guard let domain = Self.domain(of: sender) else { return nil }
        if let known = resolved[domain] { return known }
        fetch(domain)
        return nil
    }

    /// How to ask. A closure rather than a client, so this holds no
    /// credential of its own and stops working the moment the session
    /// does.
    var load: ((String) async -> Data?)?

    private func fetch(_ domain: String) {
        guard !inflight.contains(domain), let load else { return }
        inflight.insert(domain)
        Task { [weak self] in
            let data = await load(domain)
            guard let self else { return }
            inflight.remove(domain)
            // Absence is an answer worth keeping: without storing the
            // nil, every redraw asks again for a domain that has
            // already said it has nothing.
            guard let data, let uiImage = PlatformImage(data: data) else {
                resolved[domain] = Image?.none
                return
            }
            resolved[domain] = Image(platformImage: uiImage)
        }
    }

    /// The domain of the address, lowercased — the same key the web
    /// asks the endpoint with.
    static func domain(of sender: String) -> String? {
        let email = SenderName.extractEmail(sender)
        guard let at = email.lastIndex(of: "@") else { return nil }
        let domain = String(email[email.index(after: at)...])
        guard domain.contains(".") else { return nil }
        return domain
    }
}
