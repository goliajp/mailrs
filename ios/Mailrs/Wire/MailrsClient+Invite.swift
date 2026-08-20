import Foundation

extension MailrsClient {
    /// `GET /api/mail/messages/{uid}` — the invitation this message
    /// carries, and whatever answer this reader already gave.
    ///
    /// Asked only when the message's own `invite_method` says there is
    /// one: the event is a few kilobytes and every other message would
    /// pay for it.
    func invite(uid: UInt32) async throws -> Wire.MessageDetail {
        try await getJSON("/api/mail/messages/\(uid)")
    }


    /// `POST /api/invites/{uid}/rsvp` — answer it.
    ///
    /// The server records the choice **and** queues an iTIP `REPLY` to
    /// the organiser. A `202` means the choice was recorded and the
    /// reply could not be sent — which must not be reported as success,
    /// because the organiser is then still waiting.
    func rsvp(uid: UInt32, partstat: String) async throws {
        let body = try JSONSerialization.data(withJSONObject: ["partstat": partstat])
        let (data, response) = try await send(
            "POST", "/api/invites/\(uid)/rsvp", body: body, authorized: true)
        guard let http = response as? HTTPURLResponse else {
            throw MailrsError.transport("No HTTP response.")
        }
        if http.statusCode == 202 {
            let why =
                (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["message"]
                as? String
            throw MailrsError.transport(why ?? "The reply could not be sent.")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw MailrsError.server(status: http.statusCode)
        }
    }
}
