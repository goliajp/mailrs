import Foundation

/// Naming the file an attachment is written to.
///
/// Mail names its own attachments, and a name is attacker-controlled
/// input: `../../Library/Preferences/whatever` is a filename as far as
/// the sender is concerned. This reduces it to a single path component
/// so the write cannot land outside the directory it was given.
enum AttachmentFile {
    /// Longer than most filesystems accept, and every one truncates.
    private static let maxBytes = 200

    static func safeName(for filename: String) -> String {
        // `lastPathComponent` collapses any directory part, including
        // the `..` segments; the separator replacement covers what is
        // left of an encoded one.
        var base = (filename as NSString).lastPathComponent
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "\\", with: "_")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if base.isEmpty || base == "." || base == ".." { return "attachment" }
        // **Control characters, and the NUL especially.** Every POSIX
        // call underneath takes a C string, so a name containing a NUL
        // is silently cut there — `report.pdfNULL.exe` becomes
        // `report.pdf` on the way in and something else on the way out.
        base = String(base.unicodeScalars.filter { $0.value >= 0x20 && $0.value != 0x7F })
        if base.isEmpty { return "attachment" }
        return truncated(base)
    }

    /// Shortened from the **stem**, never from the end.
    ///
    /// A name cut at the end loses its extension, and Quick Look picks
    /// its renderer off the extension — so a truncated name is a file
    /// the phone cannot preview at all.
    private static func truncated(_ name: String) -> String {
        if name.utf8.count <= maxBytes { return name }
        var suffix = ""
        if let dot = name.lastIndex(of: "."), dot != name.startIndex,
            name.distance(from: dot, to: name.endIndex) <= 12
        {
            suffix = String(name[dot...])
        }
        let room = maxBytes - suffix.utf8.count
        if room <= 0 { return "attachment" }
        // By bytes, and never through a character: a name cut inside a
        // multi-byte sequence writes a filename with a replacement
        // character in it.
        var out = ""
        var used = 0
        for character in name.dropLast(suffix.count) {
            let size = String(character).utf8.count
            if used + size > room { break }
            out.append(character)
            used += size
        }
        if out.isEmpty { return "attachment" }
        return out + suffix
    }
}
