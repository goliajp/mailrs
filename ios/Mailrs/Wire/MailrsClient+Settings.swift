import Foundation

/// Calls for the reading features beside the account's settings, starting
/// with the sectioned Inbox. Every route is the one the web client calls.
@MainActor
extension MailrsClient {

    /// A JSON body sent with `method`, answered with JSON.
    func writeJSON<T: Decodable, B: Encodable>(
        _ method: String, _ path: String, body: B
    ) async throws -> T {
        let encoded = try JSONEncoder().encode(body)
        let (data, response) = try await send(method, path, body: encoded, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            let head = String(decoding: data.prefix(240), as: UTF8.self)
            throw MailrsError.decoding("\(path) — \(error) — got: \(head)")
        }
    }

    /// `POST /api/senders/accept`: these first contacts leave New, now and
    /// for mail to come. Answers how many were accepted.
    func acceptSenders(_ addresses: [String]) async throws -> Int {
        struct Body: Encodable { let addresses: [String] }
        struct Answer: Decodable { let accepted: Int }
        let answer: Answer = try await writeJSON(
            "POST", "/api/senders/accept", body: Body(addresses: addresses))
        return answer.accepted
    }
}
