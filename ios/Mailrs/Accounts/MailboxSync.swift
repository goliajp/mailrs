import Foundation

/// One pass over one account.
///
/// The orchestration, kept apart from the socket and from the rules it
/// applies: what to ask for is `FetchPlan`, what to keep is
/// `MailboxApply`, and this is the order they happen in.
enum MailboxSync {
    /// What one pass produced.
    struct Result {
        var rows: [MailboxRow]
        var marks: [String: FolderMark]
        /// Folders whose numbering changed, so their held rows are
        /// worthless and must be replaced rather than merged.
        var renumbered: Set<String>
    }

    /// Read the folders worth reading, and say what to keep.
    ///
    /// Failures of **one folder do not fail the pass**: a mailbox with
    /// twelve folders where one is broken should show the other
    /// eleven, and a person who cannot see any mail because of a
    /// folder they never open has no way to work that out.
    static func pass(
        account: MailAccount,
        session: IMAPSession,
        marks: [String: FolderMark]
    ) async throws -> Result {
        var out = Result(rows: [], marks: marks, renumbered: [])
        let folders = try await session.list()

        for folder in folders where worthReading(folder, skip: account.skipFolders) {
            do {
                let (validity, _) = try await session.select(folder.name)
                let plan = FetchPlan.decide(mark: marks[folder.name], serverValidity: validity)
                if plan == .renumbered { out.renumbered.insert(folder.name) }

                let fetched = try await session.fetchHeaders(range: plan.range)
                var highest = plan == .renumbered ? 0 : (marks[folder.name]?.highestUid ?? 0)
                for message in fetched {
                    out.rows.append(
                        MailboxRow(
                            accountId: account.id,
                            uid: message.uid,
                            folder: folder.name,
                            seen: message.seen,
                            sender: MessageHeaders.senderName(message.headers.from),
                            subject: message.headers.subject,
                            date: message.date,
                            messageId: message.headers.messageId))
                    highest = max(highest, message.uid)
                }
                // Written **after** the rows are in hand, not before:
                // a mark saved for messages that were never kept skips
                // them for good, and nothing afterwards would ask for
                // them again.
                out.marks[folder.name] = FolderMark(uidValidity: validity, highestUid: highest)
            } catch {
                continue
            }
        }
        return out
    }

    /// Whether a folder is worth reading.
    ///
    /// `\Noselect` cannot be opened at all — it is a node in the tree
    /// rather than a mailbox. A provider's view holding a copy of
    /// everything doubles every message, and its Trash and Spam are
    /// the two a person would skip themselves.
    static func worthReading(
        _ folder: (name: String, attributes: [String]), skip: [String]
    ) -> Bool {
        let attributes = Set(folder.attributes.map { $0.uppercased() })
        if attributes.contains("\\NOSELECT") { return false }
        if attributes.contains("\\ALL") || attributes.contains("\\TRASH")
            || attributes.contains("\\JUNK")
        {
            return false
        }
        return !skip.contains { $0.caseInsensitiveCompare(folder.name) == .orderedSame }
    }
}
