import Foundation

/// Whether a conversation is asleep right now.
///
/// Its own function because the answer is a comparison against the
/// clock, and a view that inlines `row.snoozedUntil ?? 0 > Date()...`
/// is a view with a rule in it that nothing can test. A server older
/// than v2.55 does not send the field at all, and absent means awake —
/// not "asleep since 1970".
enum SnoozeState {
    static func isAsleep(_ conversation: Wire.Conversation, now: Date) -> Bool {
        guard let until = conversation.snoozedUntil, until > 0 else { return false }
        return Double(until) > now.timeIntervalSince1970
    }
}
