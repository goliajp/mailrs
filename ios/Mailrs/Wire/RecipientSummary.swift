import Foundation

/// The short form of a long `To:` line.
///
/// A mailing that names several hundred addressees in `To:` would
/// otherwise be one line truncated in the middle, which says neither
/// that the reader is on it nor how many others are. Above
/// `collapseAbove` addressees the header shows the reader, the first
/// few names and a count, and the whole list only when asked.
struct RecipientSummary: Equatable {
    /// Eight short names already fill about two lines; a list longer
    /// than that is a mailing. The web client uses the same number.
    static let collapseAbove = 8
    private static let namedInSummary = 3

    /// The reader is on the list. Not true of a Bcc, which the header
    /// does not show.
    let me: Bool
    let named: [String]
    /// Addressees neither named nor the reader.
    let rest: Int
    let total: Int

    /// `nil` when the list is short enough to show whole.
    static func of(_ recipients: String, myAddress: String) -> RecipientSummary? {
        let entries = MailAddresses.split(recipients)
        guard entries.count > collapseAbove else { return nil }
        let mine = myAddress.trimmingCharacters(in: .whitespaces).lowercased()
        var mineCount = 0
        var named: [String] = []
        for entry in entries {
            if !mine.isEmpty, MailAddresses.bare(entry) == mine {
                mineCount += 1
                continue
            }
            if named.count < namedInSummary {
                named.append(SenderName.extractName(entry))
            }
        }
        return RecipientSummary(
            me: mineCount > 0, named: named,
            rest: entries.count - mineCount - named.count, total: entries.count)
    }
}
