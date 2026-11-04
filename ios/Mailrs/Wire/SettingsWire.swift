import Foundation

/// The reader's own settings: push, signed-in sessions, app passwords, the
/// vacation reply, masked addresses and where a sender's mail is filed.
/// Every route is the one the web's settings page calls.
enum SettingsWire {
    /// Backend: `core-sidestate/families/notify.rs` — `Prefs`.
    struct NotifyPrefs: Codable, Equatable, Sendable {
        /// `default`, `all`, `important` or `off`.
        var mode: String
        /// Minutes after local midnight; both set or neither.
        var quietStart: Int?
        var quietEnd: Int?
        var utcOffsetMinutes: Int
        var hideContent: Bool

        enum CodingKeys: String, CodingKey {
            case mode
            case quietStart = "quiet_start"
            case quietEnd = "quiet_end"
            case utcOffsetMinutes = "utc_offset_minutes"
            case hideContent = "hide_content"
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            mode = try c.decodeIfPresent(String.self, forKey: .mode) ?? "default"
            quietStart = try c.decodeIfPresent(Int.self, forKey: .quietStart)
            quietEnd = try c.decodeIfPresent(Int.self, forKey: .quietEnd)
            utcOffsetMinutes = try c.decodeIfPresent(Int.self, forKey: .utcOffsetMinutes) ?? 0
            hideContent = try c.decodeIfPresent(Bool.self, forKey: .hideContent) ?? false
        }
    }

    /// Backend: `handlers/notify.rs` — `Overrides`, pairs of key and choice.
    struct Overrides: Decodable, Equatable, Sendable {
        let senders: [[String]]
        let threads: [[String]]
    }

    /// Backend: `handlers/auth/sessions.rs` — `SessionView`.
    struct SignedInSession: Decodable, Identifiable, Equatable, Sendable {
        let id: String
        let createdAt: Int64
        let lastSeen: Int64
        let ip: String
        let userAgent: String
        let current: Bool

        enum CodingKeys: String, CodingKey {
            case id, ip, current
            case createdAt = "created_at"
            case lastSeen = "last_seen"
            case userAgent = "user_agent"
        }
    }

    /// Backend: `handlers/app_passwords.rs` — `list`.
    struct AppPassword: Decodable, Identifiable, Equatable, Sendable {
        let id: String
        let name: String
        let createdAt: Int64
        let lastUsedAt: Int64?

        enum CodingKeys: String, CodingKey {
            case id, name
            case createdAt = "created_at"
            case lastUsedAt = "last_used_at"
        }
    }

    /// Backend: `core-sidestate/families/vacation.rs` — `Vacation`. Sent
    /// back whole, as read, with only `enabled` changed.
    struct Vacation: Codable, Equatable, Sendable {
        var enabled: Bool
        var subject: String
        var body: String
        var start: Int64?
        var end: Int64?
        var days: Int
        var version: Int64

        /// Turning it on with no text is refused (`invalid_body`), so the
        /// phone does not offer it: the text is written on the web.
        var canTurnOn: Bool { !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    /// Backend: `core-sidestate/families/masked_email` — `Mask`, camelCase.
    struct Mask: Decodable, Identifiable, Equatable, Sendable {
        let id: String
        let email: String
        let state: String
        let forDomain: String?
        let description: String?
    }

    /// The buckets a sender's mail can always be filed to.
    enum Bucket: String, CaseIterable, Identifiable, Sendable {
        case inbox, notifications, promotions
        var id: String { rawValue }
    }

    /// Whether remote images load: the web's `mailrs_remote_images`.
    enum Images: String, CaseIterable, Identifiable, Sendable {
        /// Load them with the message.
        case auto
        /// Hold them until "Load images" is tapped.
        case click
        /// Never load them, and offer no button.
        case never
        var id: String { rawValue }
    }

    /// The web settings page where app passwords are made.
    static func appPasswordsPage(server: URL) -> URL {
        server.appending(path: "settings").appending(queryItems: [URLQueryItem(name: "tab", value: "security")])
    }
}

@MainActor
extension MailrsClient {
    func notifyPrefs() async throws -> SettingsWire.NotifyPrefs { try await getJSON("/api/notify") }

    func saveNotifyPrefs(_ p: SettingsWire.NotifyPrefs) async throws {
        try await sendJSON("PUT", "/api/notify", body: p)
    }

    func notifyOverrides() async throws -> SettingsWire.Overrides { try await getJSON("/api/notify/overrides") }

    /// `always`, `never`, or nil to go back to the mode's rule.
    func notifySender(_ address: String, _ value: String?) async throws {
        try await sendJSON("PUT", "/api/notify/senders/\(MailrsClient.segment(address))", body: OverrideBody(value: value))
    }

    /// `follow`, `mute`, or nil to go back to the mode's rule.
    func notifyThread(_ threadId: String, _ value: String?) async throws {
        try await sendJSON("PUT", "/api/notify/threads/\(MailrsClient.segment(threadId))", body: OverrideBody(value: value))
    }

    func signedInSessions() async throws -> [SettingsWire.SignedInSession] { try await getJSON("/api/auth/sessions") }

    func endSession(_ id: String) async throws {
        try await verb("DELETE", "/api/auth/sessions/\(MailrsClient.segment(id))")
    }

    func endOtherSessions() async throws -> Int {
        struct Answer: Decodable { let ended: Int }
        struct Empty: Encodable {}
        let a: Answer = try await writeJSON("POST", "/api/auth/sessions/end-others", body: Empty())
        return a.ended
    }

    func appPasswords() async throws -> [SettingsWire.AppPassword] { try await getJSON("/api/auth/app-passwords") }

    func revokeAppPassword(_ id: String) async throws {
        try await verb("DELETE", "/api/auth/app-passwords/\(MailrsClient.segment(id))")
    }

    func vacation() async throws -> SettingsWire.Vacation { try await getJSON("/api/settings/vacation") }

    func saveVacation(_ v: SettingsWire.Vacation) async throws -> SettingsWire.Vacation {
        try await writeJSON("PUT", "/api/settings/vacation", body: v)
    }

    func masks() async throws -> [SettingsWire.Mask] { try await getJSON("/api/masked-emails") }

    func createMask(forDomain: String, description: String) async throws -> SettingsWire.Mask {
        struct Body: Encodable { let forDomain: String; let description: String }
        return try await writeJSON("POST", "/api/masked-emails", body: Body(forDomain: forDomain, description: description))
    }

    /// Files the sender's conversations to `bucket` now and from now on.
    /// Answers how many moved.
    func fileSender(_ sender: String, to bucket: SettingsWire.Bucket) async throws -> Int {
        struct Body: Encodable { let sender: String; let bucket: String }
        struct Answer: Decodable { let moved: Int }
        let a: Answer = try await writeJSON("POST", "/api/senders/reclassify", body: Body(sender: sender, bucket: bucket.rawValue))
        return a.moved
    }

    /// A JSON body whose answer is empty (`204`).
    private func sendJSON<B: Encodable>(_ method: String, _ path: String, body: B) async throws {
        let (_, response) = try await send(method, path, body: try JSONEncoder().encode(body), authorized: true)
        guard let http = response as? HTTPURLResponse else { throw MailrsError.transport("No HTTP response.") }
        guard (200..<300).contains(http.statusCode) else { throw MailrsError.server(status: http.statusCode) }
    }
}

struct OverrideBody: Encodable {
    let value: String?
    enum CodingKeys: String, CodingKey { case value }

    // written out as `null`, the way the web sends it; the server reads an absent value the same way
    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(value, forKey: .value)
    }
}
