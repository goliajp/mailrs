import Foundation

/// Whether a compose session is worth keeping.
///
/// Opening the composer and closing it again must not leave a draft
/// behind — an empty one is litter that has to be found and deleted
/// later, and a Drafts list full of blanks is worse than no list.
enum DraftRule {
    static func isWorthSaving(to: String, subject: String, body: String) -> Bool {
        [to, subject, body].contains { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }

    /// How a draft reads in a list. The subject if it has one, else the
    /// first line of the body, else something that says it is empty
    /// rather than showing nothing at all.
    static func title(subject: String, body: String) -> String {
        let trimmedSubject = subject.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedSubject.isEmpty { return trimmedSubject }
        let firstLine = body
            .split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            .first
            .map(String.init)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return firstLine.isEmpty ? "(no subject)" : firstLine
    }
}
