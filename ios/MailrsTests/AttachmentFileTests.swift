import Testing

@testable import Mailrs

/// A filename off the wire is attacker-controlled: the sender chose it.
struct AttachmentFileTests {
    @Test func keepsAnOrdinaryName() {
        #expect(AttachmentFile.safeName(for: "invoice.pdf") == "invoice.pdf")
        #expect(AttachmentFile.safeName(for: "請求書_2026年8月分.xlsx") == "請求書_2026年8月分.xlsx")
    }

    @Test(arguments: [
        "../../../etc/passwd",
        "/etc/passwd",
        "..\\..\\windows\\system32\\config",
        "subdir/nested.pdf",
    ])
    func reducesAPathToOneComponent(name: String) {
        let safe = AttachmentFile.safeName(for: name)
        #expect(!safe.contains("/"))
        #expect(!safe.contains("\\"))
        #expect(safe != "..")
    }

    @Test(arguments: ["", "   ", ".", ".."])
    func fallsBackWhenThereIsNoUsableName(name: String) {
        #expect(AttachmentFile.safeName(for: name) == "attachment")
    }

    /// The extension has to survive: Quick Look picks its renderer off it,
    /// so a sanitiser that stripped it would turn every attachment into
    /// an unpreviewable blob.
    @Test func keepsTheExtension() {
        #expect(AttachmentFile.safeName(for: "../report.pdf").hasSuffix(".pdf"))
    }
}
