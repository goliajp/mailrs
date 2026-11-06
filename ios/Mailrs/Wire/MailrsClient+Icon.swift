import Foundation

/// Sender icons. Split out of `MailrsClient+Mail.swift` at the 500-line
/// limit.
extension MailrsClient {
    /// `GET /api/icon/{domain}?format=png` — the sender-avatar cascade.
    /// Backend: `crates/webapi/src/handlers/icon/mod.rs`. Bytes on a hit,
    /// **204 on a miss** rather than 404, so "this domain has no
    /// icon" is an answer rather than an error.
    func icon(domain: String) async throws -> Data? {
        let url = try Self.iconURL(base: baseURL, domain: domain)
        let (data, response) = try await send("GET", url: url, body: nil, authorized: true)
        guard let http = response as? HTTPURLResponse else { return nil }
        guard http.statusCode == 200, !data.isEmpty else { return nil }
        return data
    }

    /// Asks for a PNG: BIMI logos are SVG and some favicons ICO, and
    /// `UIImage` / `NSImage` decode neither, so without it those senders
    /// show a letter. The web asks without `format` and keeps the SVG.
    static func iconURL(base: URL, domain: String) throws -> URL {
        let encoded = domain.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? domain
        var components = URLComponents(
            url: base.appendingPathComponent("/api/icon/\(encoded)"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [URLQueryItem(name: "format", value: "png")]
        guard let url = components?.url else { throw MailrsError.transport("Bad icon URL") }
        return url
    }
}
