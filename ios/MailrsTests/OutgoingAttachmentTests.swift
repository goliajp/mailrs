import Foundation
import Testing

@testable import Mailrs

/// Sending a file, checked by reading it back.
///
/// The builder and the parser are two halves of this app that never
/// meet in production — one writes what leaves, the other reads what
/// arrives — so pointing one at the other is the only check that the
/// message this app sends is a message this app could receive.
@Suite struct OutgoingAttachmentTests {
    private let when = Date(timeIntervalSince1970: 1_756_000_000)
    private let utc = TimeZone(identifier: "UTC")!

    private func built(_ attachments: [OutgoingMessage.Attachment] = []) -> String {
        var draft = OutgoingMessage.Draft(from: "me@example.com", to: ["you@example.com"])
        draft.subject = "Here it is"
        draft.body = "See attached."
        draft.attachments = attachments
        return OutgoingMessage.text(draft, id: "x@example.com", date: when, timeZone: utc)
    }

    /// No attachments is the plain message it always was.
    @Test func aMessageWithNoAttachmentIsNotMultipart() {
        let message = built()
        #expect(message.contains("Content-Type: text/plain; charset=utf-8"))
        #expect(!message.contains("multipart"))
    }

    /// **Read back with this app's own parser.** The bytes that come
    /// out must be the bytes that went in — base64, line wrapping,
    /// boundaries and all.
    @Test func anAttachmentSurvivesBeingWrittenAndRead() {
        let payload = Data((0..<5000).map { UInt8($0 % 251) })
        let message = built([
            .init(filename: "report 2025.pdf", mimeType: "application/pdf", bytes: payload)
        ])
        let found = MessageAttachments.of(SocketText.bytes(message))
        #expect(found.count == 1)
        #expect(found.first?.filename == "report 2025.pdf")
        #expect(found.first?.mimeType == "application/pdf")
        #expect(found.first?.bytes == payload)
    }

    /// **The text part comes first.** Every reader shows the first
    /// text part it finds, and a message whose first part is a PDF
    /// opens as a PDF with the words underneath it.
    @Test func theWordsAreStillTheBody() {
        let message = built([
            .init(filename: "a.pdf", mimeType: "application/pdf", bytes: Data([1, 2, 3]))
        ])
        let body = MessageBody.extract(SocketText.bytes(message))
        #expect(body.text == "See attached.\r\n")
        #expect(body.isHTML == false)
    }

    /// Several files come back as several files, in order.
    @Test func twoAttachmentsAreTwoAttachments() {
        let message = built([
            .init(filename: "one.txt", mimeType: "text/plain", bytes: Data("first".utf8)),
            .init(
                filename: "two.bin", mimeType: "application/octet-stream",
                bytes: Data([0, 1])),
        ])
        let found = MessageAttachments.of(SocketText.bytes(message))
        #expect(found.map(\.filename) == ["one.txt", "two.bin"])
        #expect(String(decoding: found[0].bytes, as: UTF8.self) == "first")
    }

    /// **A filename cannot break the header it sits in.** A quote ends
    /// the quoted string early and a newline ends the header — which
    /// is how a filename becomes an injected header.
    @Test func aFilenameCannotInjectAHeader() {
        let nasty = "in\"voice\r\nBcc: someone@else.example\r\n.pdf"
        let message = built([
            .init(filename: nasty, mimeType: "application/pdf", bytes: Data([9]))
        ])
        // **The property is that no line is a header it should not
        // be** — not that the letters are absent. The name keeps
        // `Bcc:` as text once the newlines are stripped, which is
        // fine: a filename is allowed to contain a colon.
        let lines = message.components(separatedBy: "\r\n")
        #expect(
            !lines.contains { $0.lowercased().hasPrefix("bcc:") },
            "a header was injected through a filename")
        #expect(lines.filter { $0.hasPrefix("Content-Disposition:") }.count == 1)
        #expect(MessageAttachments.of(SocketText.bytes(message)).count == 1)
    }

    /// The boundary is derived from the message id, which is already
    /// unique — a boundary that turns up inside a part cuts the
    /// message in half at that point.
    @Test func theBoundaryDoesNotAppearInsideTheMessage() {
        let message = built([
            .init(
                filename: "a.bin", mimeType: "application/octet-stream",
                bytes: Data(count: 3000))
        ])
        let boundary = message.components(separatedBy: "boundary=\"")[1]
            .components(separatedBy: "\"")[0]
        // Three: the two parts and the close.
        #expect(message.components(separatedBy: "--" + boundary).count - 1 == 3)
    }

    /// Base64 is wrapped, as RFC 2045 asks, and still decodes.
    @Test func base64IsWrappedAtSeventySix() {
        let message = built([
            .init(
                filename: "a.bin", mimeType: "application/octet-stream",
                bytes: Data(count: 1000))
        ])
        let longest = message.components(separatedBy: "\r\n").map(\.count).max() ?? 0
        #expect(longest <= 78)
    }
}
