import Foundation

/// Many threads, one request.
///
/// `POST /api/conversations/batch` takes `{action, thread_ids}` and
/// answers how many went through **and which ones did not**. The
/// per-row loop this replaced existed for a reason — it was the only
/// way to learn which rows failed — and that reason went away when the
/// route started naming them.
@MainActor
extension MailrsClient {

    func batch(action: String, threadIds: [String]) async throws -> Set<String> {
        struct Request: Encodable {
            let action: String
            let threadIds: [String]

            enum CodingKeys: String, CodingKey {
                case action
                case threadIds = "thread_ids"
            }
        }
        struct Answer: Decodable {
            let failedThreadIds: [String]?

            enum CodingKeys: String, CodingKey {
                case failedThreadIds = "failed_thread_ids"
            }
        }
        let body = try JSONEncoder().encode(Request(action: action, threadIds: threadIds))
        let (data, response) = try await send(
            "POST", "/api/conversations/batch", body: body, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        let answer = try? JSONDecoder().decode(Answer.self, from: data)
        return Set(answer?.failedThreadIds ?? [])
    }
}
