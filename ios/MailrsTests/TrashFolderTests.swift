import Testing

@testable import Mailrs

/// Which folder a deleted message goes to.
@Suite struct TrashFolderTests {
    private func folder(_ name: String, _ attributes: [String] = [])
        -> (name: String, attributes: [String])
    {
        (name, attributes)
    }

    /// The `\Trash` attribute is the server saying it in so many
    /// words, and it is right regardless of language.
    @Test func theSpecialUseAttributeWins() {
        let folders = [folder("Trash"), folder("ゴミ箱", ["\\HasNoChildren", "\\Trash"])]
        #expect(TrashFolder.pick(folders) == "ゴミ箱")
    }

    /// Servers that do not publish it fall back to the usual names.
    @Test func theUsualNamesAreRecognised() {
        #expect(TrashFolder.pick([folder("INBOX"), folder("Trash")]) == "Trash")
        #expect(TrashFolder.pick([folder("INBOX"), folder("Deleted Items")]) == "Deleted Items")
        #expect(TrashFolder.pick([folder("Deleted Messages")]) == "Deleted Messages")
    }

    /// `[Gmail]/Trash` and `INBOX.Trash` are both called Trash.
    @Test func aNestedTrashIsStillTrash() {
        #expect(TrashFolder.pick([folder("[Gmail]/Trash")]) == "[Gmail]/Trash")
        #expect(TrashFolder.pick([folder("INBOX.Trash")]) == "INBOX.Trash")
    }

    /// **Nil means do not delete.** Guessing a name and having the
    /// server create it puts the message somewhere no other client the
    /// person uses will ever look.
    @Test func noTrashFolderMeansNoDeleting() {
        #expect(TrashFolder.pick([folder("INBOX"), folder("Archive")]) == nil)
        #expect(TrashFolder.pick([]) == nil)
    }

    /// A folder whose name merely contains the word is not it.
    @Test func aFolderThatOnlyMentionsTrashIsNotTrash() {
        #expect(TrashFolder.pick([folder("Trashy ideas"), folder("Not trash")]) == nil)
    }
}
