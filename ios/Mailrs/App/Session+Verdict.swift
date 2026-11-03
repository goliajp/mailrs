import Foundation

/// The stored fraud verdict of one message, and putting a held
/// conversation back.
@MainActor
extension Session {

    /// The verdict, or nil for mail nobody suspected and nothing sorted —
    /// and for a server that could not say, which the reading page treats
    /// the same way: it draws no panel rather than an alarming one.
    func fraudVerdict(messageId: String) async -> Wire.FraudVerdict? {
        guard let client, !messageId.isEmpty else { return nil }
        return try? await client.fraudVerdict(messageId: messageId)
    }

    /// "This is not fraud": the conversation leaves Review and goes back
    /// where it was. True when the server took it.
    func releaseHeld(threadId: String) async -> Bool {
        guard let client else { return false }
        do {
            try await client.release(threadId: threadId)
        } catch {
            banner = error.localizedDescription
            return false
        }
        _ = removeRows([threadId])
        return true
    }
}
