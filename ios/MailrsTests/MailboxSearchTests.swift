import Testing

@testable import Mailrs

/// Finding a message in what has already been fetched.
@Suite struct MailboxSearchTests {
    private func row(_ sender: String, _ subject: String, folder: String = "INBOX")
        -> MailboxRow
    {
        MailboxRow(
            accountId: "a", uid: UInt32(abs(sender.hashValue % 1000)), folder: folder,
            seen: false, sender: sender, subject: subject, date: nil, messageId: "m")
    }

    private var rows: [MailboxRow] {
        [
            row("Ada <ada@example.com>", "Lunch on Thursday"),
            row("Bob <bob@example.com>", "Invoice 4471"),
            row("Ada <ada@example.com>", "Re: Invoice 4471"),
            row("会議事務局 <mtg@example.jp>", "会議のお知らせ"),
        ]
    }

    /// Nothing typed is not a filter.
    @Test func anEmptyQueryMatchesEverything() {
        #expect(MailboxSearch.matches(rows, "").count == rows.count)
        #expect(MailboxSearch.matches(rows, "   ").count == rows.count)
    }

    /// Case is not something anybody types deliberately.
    @Test func matchingIgnoresCase() {
        #expect(MailboxSearch.matches(rows, "ADA").count == 2)
        #expect(MailboxSearch.matches(rows, "invoice").count == 2)
    }

    /// **Every** word, not any: somebody typing two words is
    /// narrowing, and a search that widens with each word gets further
    /// from what they want the more they say.
    @Test func everyWordMustMatch() {
        #expect(MailboxSearch.matches(rows, "ada invoice").count == 1)
        #expect(MailboxSearch.matches(rows, "ada lunch invoice").isEmpty)
    }

    /// The words may match different fields — "ada lunch" is a message
    /// from Ada about lunch, which is how people search and not how a
    /// naive substring match behaves.
    @Test func wordsMayMatchDifferentFields() {
        let found = MailboxSearch.matches(rows, "ada lunch")
        #expect(found.count == 1)
        #expect(found.first?.subject == "Lunch on Thursday")
    }

    /// No spaces between words is not a reason to find nothing.
    @Test func cjkMatchesAsASubstring() {
        #expect(MailboxSearch.matches(rows, "会議").count == 1)
        #expect(MailboxSearch.matches(rows, "お知らせ").count == 1)
    }

    /// The folder is searchable too — it is on the row and on screen.
    @Test func theFolderNameIsSearchable() {
        let filed = rows + [row("Carol <c@example.com>", "Receipt", folder: "Archive")]
        #expect(MailboxSearch.matches(filed, "archive").count == 1)
    }
}
