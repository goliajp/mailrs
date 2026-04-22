import Foundation

struct Draft: Codable, Identifiable, Sendable {
    let id: Int
    let to_addresses: String
    let cc_addresses: String
    let bcc_addresses: String
    let subject: String
    let body: String
    let reply_to_thread_id: String?
    let created_at: String
    let updated_at: String
}

struct SaveDraftRequest: Encodable, Sendable {
    let to: String?
    let cc: String?
    let bcc: String?
    let subject: String?
    let body: String?
    let reply_to_thread_id: String?
}

struct SaveDraftResult: Decodable, Sendable {
    let success: Bool
    let id: Int?
    let message: String?
}

struct DraftService {
    let api: ApiClient

    func list() async throws -> [Draft] {
        try await api.get("/api/mail/drafts")
    }

    func save(_ request: SaveDraftRequest) async throws -> SaveDraftResult {
        try await api.post("/api/mail/drafts", body: request)
    }

    func delete(id: Int) async throws {
        let _: EmptyResponse = try await api.delete("/api/mail/drafts/\(id)")
    }
}
