import Foundation

/// Whether the Inbox is shown by section (All, To reply, Waiting, New
/// senders). The reader's setting, kept on the server
/// (`/api/settings/sectioned-inbox`) so the web and both apps agree.
///
/// Backend: `core-sidestate/families/sectioned_inbox.rs` — `SectionedInbox`.
struct SectionedInboxSetting: Codable, Equatable, Sendable {
    var enabled: Bool

    /// On until the reader turns it off, the server's default.
    static let standard = SectionedInboxSetting(enabled: true)
}

extension MailrsClient {
    func sectionedInboxSetting() async throws -> SectionedInboxSetting {
        try await getJSON("/api/settings/sectioned-inbox")
    }

    func saveSectionedInboxSetting(_ s: SectionedInboxSetting) async throws -> SectionedInboxSetting {
        try await writeJSON("PUT", "/api/settings/sectioned-inbox", body: s)
    }
}
