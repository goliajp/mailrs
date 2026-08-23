import Foundation

/// Which folder a deleted message goes to.
///
/// There is no single answer, which is the whole point of this file.
/// Gmail calls it `[Gmail]/Trash`, Outlook `Deleted Items`, iCloud
/// `Deleted Messages`, and a server in Japan may call it `ゴミ箱`. A
/// client that hard-codes `Trash` deletes nothing on three of those
/// and **creates a folder called Trash** on some of them, where the
/// message then sits invisible to every other client the person uses.
enum TrashFolder {
    /// Names to fall back on, for servers that do not publish
    /// special-use. Matched case-insensitively and by the last path
    /// segment, so `[Gmail]/Trash` and `INBOX.Trash` both count.
    private static let known = [
        "trash", "deleted items", "deleted messages", "bin", "ゴミ箱", "已删除邮件", "垃圾桶",
    ]

    /// - Parameter folders: every folder the server listed, with
    ///   attributes.
    /// - Returns: the folder to move to, or nil — and **nil means do
    ///   not delete**. Guessing a name and having the server create it
    ///   puts the message somewhere no other client will look.
    static func pick(_ folders: [(name: String, attributes: [String])]) -> String? {
        // The `\Trash` attribute (RFC 6154) is the server saying it in
        // so many words, and it is right regardless of language. It is
        // asked first for that reason.
        if let special = folders.first(where: {
            $0.attributes.contains { $0.uppercased() == "\\TRASH" }
        }) {
            return special.name
        }
        for name in known {
            if let match = folders.first(where: { lastSegment($0.name).lowercased() == name }) {
                return match.name
            }
        }
        return nil
    }

    /// `[Gmail]/Trash` and `INBOX.Trash` are both called Trash.
    private static func lastSegment(_ name: String) -> String {
        let parts = name.split(whereSeparator: { $0 == "/" || $0 == "." })
        return parts.last.map(String.init) ?? name
    }
}
