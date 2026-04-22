import Foundation

struct ConversationSummary: Codable, Identifiable, Hashable, Sendable {
    var thread_id: String
    var subject: String
    var snippet: String
    var last_sender: String
    var last_date: Date
    var message_count: Int
    var unread_count: Int
    var participants: [String]
    var category: String
    var importance_level: String
    var importance_score: Double
    var flagged: Bool
    var pinned: Bool
    var archived: Bool
    var requires_action: Bool

    var id: String { thread_id }
    var isUnread: Bool { unread_count > 0 }
    var lastDateUnixSeconds: Int64 { Int64(last_date.timeIntervalSince1970) }
}
