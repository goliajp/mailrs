import Foundation

/// Finding a message in what has already been fetched.
///
/// Local, over the headers on the device — not `IMAP SEARCH`. Two
/// reasons: it answers while somebody is still typing, and it works on
/// a train. What it cannot do is find a message this device has never
/// seen, and the screen says so rather than letting an empty result
/// read as "you have no such mail".
enum MailboxSearch {
    /// Rows matching every word of `query`.
    ///
    /// **Every** word, not any: somebody typing two words is
    /// narrowing, and a search that widens with each word typed gets
    /// further from what they want the more they say.
    ///
    /// The words may match different fields — "ada lunch" finds a
    /// message from Ada about lunch, which is how people search and
    /// not how a naive substring match behaves.
    static func matches(_ rows: [MailboxRow], _ query: String) -> [MailboxRow] {
        let words = query.lowercased().split(separator: " ").map(String.init)
        guard !words.isEmpty else { return rows }
        return rows.filter { row in
            let haystack = "\(row.sender) \(row.subject) \(row.folder)".lowercased()
            return words.allSatisfy { haystack.contains($0) }
        }
    }
}
