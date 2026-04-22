import Foundation

struct AttachmentInfo: Codable, Hashable, Sendable {
    let filename: String
    let content_type: String
    let size: Int
}

struct ThreadMessage: Codable, Identifiable, Hashable, Sendable {
    let uid: Int
    let message_id: String
    let subject: String
    let sender: String
    let recipients: String
    let internal_date: Date
    let flags: Int

    let html_body: String?
    let text_body: String?
    let clean_text: String?
    let new_content: String?

    let attachments: [AttachmentInfo]

    let category: String
    let importance_level: String
    let importance_score: Double
    let requires_action: Bool
    let risk_score: Double
    let risk_reason: String
    let summary: String
    let sender_intent: String

    let has_tracking_pixel: Bool
    let is_bulk_sender: Bool

    let ai_analyzed: Bool?
    let bimi_logo_url: String?
    let action_deadline: String?
    let action_items: [String]?

    // Optional analysis payload — decoded leniently, rendered if simple.
    let structured_data: StructuredData?

    var id: Int { uid }

    // MARK: flag helpers (matching FLAG_* in TS)
    static let flagSeen = 1
    static let flagAnswered = 2
    static let flagFlagged = 4
    static let flagDeleted = 8
    static let flagDraft = 16

    var isSeen: Bool { flags & Self.flagSeen != 0 }
    var isFlagged: Bool { flags & Self.flagFlagged != 0 }
    var isAnswered: Bool { flags & Self.flagAnswered != 0 }

    var preferredBodyText: String {
        clean_text ?? text_body ?? ""
    }
}

struct StructuredData: Codable, Hashable, Sendable {
    let actions: [ActionItem]?
    let events: [EventInfo]?
    let orders: [Order]?
    let reservations: [Reservation]?
}

struct ActionItem: Codable, Hashable, Sendable {
    let name: String
    let type: String
    let url: String?
}

struct EventInfo: Codable, Hashable, Sendable {
    let name: String
    let start_date: String?
    let end_date: String?
    let location: String?
    let url: String?
}

struct Order: Codable, Hashable, Sendable {
    let currency: String?
    let items: [OrderItem]
    let merchant: String?
    let order_date: String?
    let order_number: String?
    let status: String?
    let total: String?
}

struct OrderItem: Codable, Hashable, Sendable {
    let name: String
    let price: String?
    let quantity: Int?
}

struct Reservation: Codable, Hashable, Sendable {
    let type: String
    let name: String?
    let provider: String?
    let reservation_id: String?
    let start_date: String?
    let end_date: String?
    let location: String?
    let flight_number: String?
    let departure_airport: String?
    let arrival_airport: String?
    let status: String?
}
