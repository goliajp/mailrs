import Foundation
import Testing

@testable import Mailrs

/// A name from a stranger, made safe to write to disk.
///
/// These were written for a connected mailbox's attachments and then
/// pointed at the function that already existed for this app's own —
/// one question, one answer. Two of them were red when they arrived:
/// the old rule handled `..` and the separators and did **not** handle
/// a NUL or a name too long for the filesystem.
///
/// An attachment's filename is attacker-controlled: it arrives in a
/// header written by whoever sent the message. This has been a real bug
/// in real mail clients more than once.
@Suite struct AttachmentFileNameTests {
    /// An ordinary name is left exactly as it was.
    @Test func anOrdinaryNameIsUntouched() {
        #expect(AttachmentFile.safeName(for: "report 2025.pdf") == "report 2025.pdf")
        #expect(AttachmentFile.safeName(for: "日本.pdf") == "日本.pdf")
    }

    /// **The property is that the result never escapes the
    /// directory** — not that a particular string comes out. The
    /// existing rule collapses `../../x.plist` to `x.plist`, which is
    /// safe and keeps the name the sender meant as the leaf; refusing
    /// outright would be safe too and would throw the name away. The
    /// assertion is on the property, so either implementation passes
    /// and neither can drift into being unsafe.
    @Test func nothingEscapesTheDirectory() {
        let attacks = [
            "../../../Library/Preferences/x.plist",
            "/etc/passwd",
            "..\\windows\\system32",
            "..",
            ".",
            "a/../../b",
        ]
        // Resolved on both sides: `/tmp` is a symlink to `/private/tmp`,
        // so comparing a standardised path against an unresolved prefix
        // fails on a file that is exactly where it should be.
        let directory = URL(fileURLWithPath: "/tmp/box").resolvingSymlinksInPath()
        for attack in attacks {
            let safe = AttachmentFile.safeName(for: attack)
            #expect(!safe.contains("/"), Comment(rawValue: attack))
            #expect(!safe.contains("\\"), Comment(rawValue: attack))
            #expect(safe != "." && safe != "..", Comment(rawValue: attack))
            let written = directory.appendingPathComponent(safe).standardized
            #expect(
                written.path.hasPrefix(directory.path + "/"),
                Comment(rawValue: "\(attack) landed at \(written.path)"))
        }
    }

    /// A directory part is dropped and the leaf is kept — which is
    /// the name the sender meant.
    @Test func aDirectoryPartIsDropped() {
        #expect(AttachmentFile.safeName(for: "holiday/photo.jpg") == "photo.jpg")
    }

    /// The \u{0} that truncates a name in every C API underneath, and the
    /// control characters that make a name unprintable.
    @Test func controlCharactersAreRemoved() {
        #expect(AttachmentFile.safeName(for: "safe\u{0}.pdf") == "safe.pdf")
        #expect(AttachmentFile.safeName(for: "a\u{9}b\u{7F}.txt") == "ab.txt")
        #expect(AttachmentFile.safeName(for: "\u{0}\u{0}") == "attachment")
    }

    /// A leading dot is **kept**.
    ///
    /// It was tempting to strip it — a dotfile hides from a file
    /// manager — but this file goes to the temporary directory and
    /// straight to Quick Look, where nobody browses for it. Stripping
    /// would rename a `.gitignore` somebody attached on purpose, and
    /// renaming what a person sent is worse than a name that would
    /// have been hidden somewhere this file never goes.
    @Test func aLeadingDotIsKept() {
        #expect(AttachmentFile.safeName(for: ".bashrc") == ".bashrc")
        // `.` and `..` are still refused: those are directories.
        #expect(AttachmentFile.safeName(for: ".") == "attachment")
        #expect(AttachmentFile.safeName(for: "..") == "attachment")
    }

    /// Nothing at all is the fallback, and the fallback is nameable.
    @Test func anEmptyNameBecomesTheFallback() {
        #expect(AttachmentFile.safeName(for: "") == "attachment")
        #expect(AttachmentFile.safeName(for: "   ") == "attachment")

    }

    /// Shortened from the stem, never from the end: a name cut at the
    /// end loses its extension, and a file the phone cannot tell the
    /// type of is a file nothing will open.
    @Test func aVeryLongNameKeepsItsExtension() {
        let safe = AttachmentFile.safeName(for: String(repeating: "a", count: 500) + ".pdf")
        #expect(safe.hasSuffix(".pdf"))
        #expect(safe.utf8.count <= 200)
    }

    /// And never through a character: a name cut inside a multi-byte
    /// sequence writes a filename with a replacement character in it.
    @Test func aLongMultibyteNameIsNotCutThroughACharacter() {
        let safe = AttachmentFile.safeName(for: String(repeating: "日", count: 300) + ".pdf")
        #expect(safe.utf8.count <= 200)
        #expect(!safe.contains("\u{FFFD}"))
        #expect(safe.hasSuffix(".pdf"))
    }
}
