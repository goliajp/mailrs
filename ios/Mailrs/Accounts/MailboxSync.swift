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
        /// Per folder, the flags the server reports for uids this
        /// device already held — and, by their absence from it, which
        /// of those uids the server no longer has.
        var refreshed: [String: [UInt32: Bool]] = [:]
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
        marks: [String: FolderMark],
        /// What this device already holds, per folder. Asked about
        /// again each pass so a message read or deleted on another
        /// device stops being wrong here — see `MailboxRefresh`.
        held: [String: [UInt32]] = [:]
    ) async throws -> Result {
        var out = Result(rows: [], marks: marks, renumbered: [])
        let folders = try await session.list()

        for folder in folders where worthReading(folder, skip: account.skipFolders) {
            do {
                let (validity, exists) = try await session.select(folder.name)
                let plan = FetchPlan.decide(
                    mark: marks[folder.name], serverValidity: validity, exists: exists)
                var renumbered = false
                if case .renumbered = plan { renumbered = true }
                if renumbered { out.renumbered.insert(folder.name) }

                let fetched = try await session.fetchHeaders(
                    range: plan.range, byUid: plan.byUid)
                var highest = marks[folder.name]?.highestUid ?? 0
                if renumbered { highest = 0 }
                var lowest = marks[folder.name]?.lowestUid ?? 0
                if renumbered { lowest = 0 }
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
                            messageId: message.headers.messageId,
                            size: message.size))
                    highest = max(highest, message.uid)
                    lowest = lowest == 0 ? message.uid : min(lowest, message.uid)
                }
                // Written **after** the rows are in hand, not before:
                // a mark saved for messages that were never kept skips
                // them for good, and nothing afterwards would ask for
                // them again.
                out.marks[folder.name] = FolderMark(
                    uidValidity: validity, highestUid: highest, lowestUid: lowest,
                    earlierSpan: marks[folder.name]?.earlierSpan ?? EarlierPlan.firstSpan)

                // And what happened to the ones already here. Cheap —
                // flags only — and the only way this device notices a
                // message read on a laptop or deleted from a phone.
                // Skipped on a renumbering, where the old uids mean
                // nothing and every row for the folder is replaced.
                let already = held[folder.name] ?? []
                if !renumbered, !already.isEmpty {
                    out.refreshed[folder.name] = try await session.flags(uids: already)
                }
            } catch {
                continue
            }
        }
        return out
    }
    /// Whether a folder belongs in a merged **inbox**.
    ///
    /// `\Noselect` cannot be opened at all — it is a node in the tree
    /// rather than a mailbox. A provider's view holding a copy of
    /// everything doubles every message, and its Trash and Spam are
    /// the two a person would skip themselves.
    ///
    /// **Sent and Drafts are skipped too**, and that is a decision
    /// rather than an omission: this list is what arrived. A draft is
    /// not a message at all — it has not been sent to anybody — and a
    /// copy of everything the person wrote, interleaved by date with
    /// what they received, is what every "all inboxes" view in every
    /// mail client deliberately does not show.
    ///
    /// A server with no special-use markers is read whole, which is
    /// the right default: a folder nobody has labelled is a folder
    /// somebody made, and those are where filed mail lives.
    static func worthReading(
        _ folder: (name: String, attributes: [String]), skip: [String]
    ) -> Bool {
        let upper = Set(folder.attributes.map { $0.uppercased() })
        if upper.contains("\\NOSELECT") { return false }
        let notAnInbox: Set<String> = ["\\ALL", "\\TRASH", "\\JUNK", "\\SENT", "\\DRAFTS"]
        if !upper.isDisjoint(with: notAnInbox) { return false }
        return !skip.contains { $0.lowercased() == folder.name.lowercased() }
    }
}
