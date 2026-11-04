import Foundation

/// What a team conversation's row says about who answers it.
enum TeamRow {
    /// The assignee's readable name, or nil when nobody is assigned.
    ///
    /// The wire carries the address; the row shows the part before the
    /// `@`, which is what the team calls each other by.
    static func assignee(_ c: Wire.Conversation) -> String? {
        let who = (c.assignee ?? "").trimmingCharacters(in: .whitespaces)
        if who.isEmpty { return nil }
        guard let at = who.firstIndex(of: "@") else { return who }
        if at == who.startIndex { return who }
        return String(who[..<at])
    }
}
