import Foundation

extension Wire {
    /// `POST /api/conversations/{id}/block-sender`.
    struct BlockSenderRequest: Encodable {
        let address: String
    }

    /// `BlockSenderResponse` in `crates/webapi/src/handlers/block_sender.rs`.
    struct BlockSenderResponse: Decodable, Equatable, Sendable {
        /// As stored: trimmed and lowercased.
        let address: String
        /// It was on the whitelist and was taken off; the undo puts it back.
        let wasAllowed: Bool

        enum CodingKeys: String, CodingKey {
            case address
            case wasAllowed = "was_allowed"
        }
    }

    /// `POST /api/conversations/{id}/unblock-sender`, the undo.
    struct UnblockSenderRequest: Encodable {
        let address: String
        let allow: Bool
    }
}

/// Blocking a conversation's sender from one of its messages: the address
/// goes on the blacklist (and off the whitelist) and the conversation to
/// Junk, in one call. Unblocking outside the undo is the plain blacklist
/// delete, `removeFromSenderList(.blocked, …)`.
extension MailrsClient {
    func blockSender(threadId: String, address: String) async throws -> Wire.BlockSenderResponse {
        let body = try JSONEncoder().encode(Wire.BlockSenderRequest(address: address))
        let path = "/api/conversations/\(MailrsClient.segment(threadId))/block-sender"
        let (data, response) = try await send("POST", path, body: body, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
        do {
            return try JSONDecoder().decode(Wire.BlockSenderResponse.self, from: data)
        } catch {
            throw MailrsError.decoding("\(path) — \(error)")
        }
    }

    func unblockSender(threadId: String, address: String, allow: Bool) async throws {
        let body = try JSONEncoder().encode(Wire.UnblockSenderRequest(address: address, allow: allow))
        let path = "/api/conversations/\(MailrsClient.segment(threadId))/unblock-sender"
        let (_, response) = try await send("POST", path, body: body, authorized: true)
        guard let http = response as? HTTPURLResponse,
              (200..<300).contains(http.statusCode)
        else {
            throw MailrsError.server(status: (response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }
}
