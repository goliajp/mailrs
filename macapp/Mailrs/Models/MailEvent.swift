import Foundation

enum ConnectionStatus: Sendable, Equatable {
    case offline
    case connecting
    case connected
}

struct NewMessageEvent: Decodable, Sendable, Equatable {
    let sender: String
    let snippet: String
    let subject: String
    let thread_id: String
    let user: String
}

/// Union of events we consume. `other` captures SMTP events and any future
/// types we haven't modeled; decoding never throws on an unrecognized type.
enum MailEvent: Sendable, Equatable {
    case newMessage(NewMessageEvent)
    case other(type: String)
}

extension MailEvent: Decodable {
    private enum CodingKeys: String, CodingKey { case type }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = (try? c.decode(String.self, forKey: .type)) ?? ""
        switch type {
        case "NewMessage":
            let event = try NewMessageEvent(from: decoder)
            self = .newMessage(event)
        default:
            self = .other(type: type)
        }
    }
}
