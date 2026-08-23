import Testing

@testable import Mailrs

/// Which folders a pass reads.
@Suite struct MailboxSyncTests {
    private func folder(_ name: String, _ attributes: [String] = [])
        -> (name: String, attributes: [String])
    {
        (name, attributes)
    }

    @Test func anOrdinaryFolderIsRead() {
        #expect(MailboxSync.worthReading(folder("INBOX", ["\\HasNoChildren"]), skip: []))
        #expect(MailboxSync.worthReading(folder("Work/Clients"), skip: []))
    }

    /// A node in the tree rather than a mailbox: `SELECT` on it fails,
    /// and a pass that tries loses the folders after it.
    @Test func aFolderThatCannotBeOpenedIsNotTried() {
        #expect(!MailboxSync.worthReading(folder("[Gmail]", ["\\Noselect"]), skip: []))
    }

    /// A view holding a copy of everything doubles the mailbox.
    @Test func aViewHoldingEverythingIsSkipped() {
        #expect(!MailboxSync.worthReading(folder("[Gmail]/All Mail", ["\\All"]), skip: []))
    }

    /// The two a person would skip themselves.
    @Test func theBinAndTheSpamAreLeftAlone() {
        #expect(!MailboxSync.worthReading(folder("Trash", ["\\Trash"]), skip: []))
        #expect(!MailboxSync.worthReading(folder("Spam", ["\\Junk"]), skip: []))
    }

    /// Not every server sets the attributes, so the provider table
    /// names them too — and the names are matched without case,
    /// because servers disagree about it.
    @Test func aNamedFolderIsSkippedEvenWithNoAttribute() {
        #expect(!MailboxSync.worthReading(folder("已删除"), skip: ["已删除"]))
        #expect(!MailboxSync.worthReading(folder("TRASH"), skip: ["Trash"]))
        #expect(MailboxSync.worthReading(folder("Trashy ideas"), skip: ["Trash"]))
    }
}

/// What a store keeps between passes.
@Suite struct MailboxStoreTests {
    /// Two accounts with an INBOX each keep their own place — the same
    /// mistake `MailboxRow.id` prevents in the list.
    @Test func twoAccountsKeepSeparatePlaces() {
        AccountStore.saveMarks([:])
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 1, highestUid: 10)], for: "a")
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 2, highestUid: 20)], for: "b")

        #expect(AccountStore.marks(for: "a")["INBOX"]?.highestUid == 10)
        #expect(AccountStore.marks(for: "b")["INBOX"]?.highestUid == 20)
        AccountStore.saveMarks([:])
    }

    /// Writing one account's marks must not disturb another's.
    @Test func savingOneAccountLeavesTheOthersAlone() {
        AccountStore.saveMarks([:])
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 1, highestUid: 10)], for: "a")
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 9, highestUid: 90)], for: "b")
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 1, highestUid: 11)], for: "a")

        #expect(AccountStore.marks(for: "b")["INBOX"]?.highestUid == 90)
        #expect(AccountStore.marks(for: "a")["INBOX"]?.highestUid == 11)
        AccountStore.saveMarks([:])
    }

    /// A folder that has gone from the account goes from its marks
    /// too: the write replaces the account's set rather than merging
    /// into it, or a renamed folder keeps its old place forever.
    @Test func aFolderThatIsGoneLeavesNoPlaceBehind() {
        AccountStore.saveMarks([:])
        AccountStore.saveMarks(
            ["INBOX": FolderMark(uidValidity: 1, highestUid: 10),
             "Old": FolderMark(uidValidity: 1, highestUid: 5)], for: "a")
        AccountStore.saveMarks(["INBOX": FolderMark(uidValidity: 1, highestUid: 11)], for: "a")

        #expect(AccountStore.marks(for: "a")["Old"] == nil)
        AccountStore.saveMarks([:])
    }
}
