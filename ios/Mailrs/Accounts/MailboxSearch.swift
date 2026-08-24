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
        let words = words(of: query)
        guard !words.isEmpty else { return rows }
        return rows.filter { row in
            let text = haystack(of: row)
            return words.allSatisfy { text.contains($0) }
        }
    }

    /// A query split into the words every row must match.
    static func words(of query: String) -> [String] {
        query.lowercased().split(separator: " ").map(String.init)
    }

    /// The text of a row that a search looks in.
    ///
    /// Here rather than in either caller because the **store keeps a
    /// folded copy of it** to search without loading every row, and two
    /// spellings of "what a search looks in" is two searches that agree
    /// until somebody writes a subject in an alphabet with case.
    /// `lowercased` folds all of Unicode; SQLite's `lower` folds ASCII,
    /// which is why the folding happens in this language and not in
    /// SQL.
    static func haystack(of row: MailboxRow) -> String {
        "\(row.sender) \(row.subject) \(row.folder)".lowercased()
    }
}
