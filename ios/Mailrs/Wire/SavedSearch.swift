import Foundation

/// A search the reader saved on the web. The phone runs them; making and
/// deleting them stays on the web.
/// Backend: `core-sidestate/families/saved_searches.rs` — `SavedSearch`.
struct SavedSearch: Decodable, Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let query: String
}

@MainActor
extension MailrsClient {
    func savedSearches() async throws -> [SavedSearch] { try await getJSON("/api/saved-searches") }
}
