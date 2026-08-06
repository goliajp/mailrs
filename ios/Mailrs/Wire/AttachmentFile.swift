import Foundation

/// Naming the file an attachment is written to.
///
/// Mail names its own attachments, and a name is attacker-controlled
/// input: `../../Library/Preferences/whatever` is a filename as far as
/// the sender is concerned. This reduces it to a single path component
/// so the write cannot land outside the directory it was given.
enum AttachmentFile {
    static func safeName(for filename: String) -> String {
        // `lastPathComponent` collapses any directory part, including
        // the `..` segments; the separator replacement covers what is
        // left of an encoded one.
        let base = (filename as NSString).lastPathComponent
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "\\", with: "_")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if base.isEmpty || base == "." || base == ".." { return "attachment" }
        return base
    }
}
