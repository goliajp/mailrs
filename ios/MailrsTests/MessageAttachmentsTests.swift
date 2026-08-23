import Foundation
import Testing

@testable import Mailrs

/// What is attached to a message.
@Suite struct MessageAttachmentsTests {
    private func raw(_ s: String) -> Data { Data(s.utf8) }

    /// A plain message has nothing attached, and says so with a list.
    @Test func aMessageWithNothingAttachedHasNothing() {
        #expect(MessageAttachments.of(raw("Subject: hi\r\n\r\nbody")).isEmpty)
    }

    /// The part a reader sees is not attached, and a PDF nobody can
    /// render still has to be listed — which is why this is a
    /// different question from what to show.
    @Test func theBodyIsNotAttachedAndThePdfIs() {
        let message = """
            Content-Type: multipart/mixed; boundary="b"\r
            \r
            --b\r
            Content-Type: text/plain\r
            \r
            see attached\r
            --b\r
            Content-Type: application/pdf; name="report.pdf"\r
            Content-Transfer-Encoding: base64\r
            \r
            SGVsbG8=\r
            --b--\r
            """
        let found = MessageAttachments.of(raw(message))
        #expect(found.count == 1)
        #expect(found.first?.filename == "report.pdf")
        #expect(found.first?.mimeType == "application/pdf")
        #expect(String(decoding: found.first?.bytes ?? Data(), as: UTF8.self) == "Hello")
    }

    /// A text part **with a filename** is attached — that is how a
    /// `.txt` or a `.csv` arrives, and treating it as the body shows
    /// the reader a spreadsheet instead of the message.
    @Test func aNamedTextPartIsAnAttachment() {
        let message = """
            Content-Type: multipart/mixed; boundary="b"\r
            \r
            --b\r
            Content-Type: text/plain\r
            \r
            the message\r
            --b\r
            Content-Type: text/csv\r
            Content-Disposition: attachment; filename="rows.csv"\r
            \r
            a,b\r
            --b--\r
            """
        let found = MessageAttachments.of(raw(message))
        #expect(found.count == 1)
        #expect(found.first?.filename == "rows.csv")
    }

    /// An inline image is listed anyway — a reader shown text has no
    /// other way to reach it — but marked, so the list can say which
    /// is which.
    @Test func anInlineImageIsListedAndMarked() {
        let message = """
            Content-Type: multipart/related; boundary="b"\r
            \r
            --b\r
            Content-Type: text/html\r
            \r
            <p>hi</p>\r
            --b\r
            Content-Type: image/png\r
            Content-Disposition: inline; filename="sig.png"\r
            \r
            PNG\r
            --b--\r
            """
        let found = MessageAttachments.of(raw(message))
        #expect(found.count == 1)
        #expect(found.first?.inline == true)
    }

    /// RFC 2231 is how a Japanese filename survives a header that must
    /// be ASCII. A client that does not decode it shows the person
    /// `%E6%97%A5%E6%9C%AC.pdf`.
    @Test func anEncodedFilenameIsDecoded() {
        let header = "attachment; filename*=utf-8\'\'%E6%97%A5%E6%9C%AC.pdf"
        #expect(MessageAttachments.rfc2231(header, "filename") == "日本.pdf")
    }

    /// A long name is split across numbered continuations.
    @Test func aFilenameSplitAcrossContinuationsIsRejoined() {
        let header = "attachment; filename*0*=utf-8\'\'%E6%97%A5; filename*1*=%E6%9C%AC.pdf"
        #expect(MessageAttachments.rfc2231(header, "filename") == "日本.pdf")
    }

    /// And the ordinary quoted form still works.
    @Test func aPlainQuotedFilenameWorks() {
        #expect(
            MessageAttachments.rfc2231(#"attachment; filename="report 2025.pdf""#, "filename")
                == "report 2025.pdf")
    }

    /// `Content-Type: ...; name=` is the older place and still
    /// arrives, so both are looked in.
    @Test func theOlderNameParameterIsFoundToo() {
        let header = "Content-Type: application/zip; name=\"archive.zip\"\r\n"
        #expect(MessageAttachments.filename(header) == "archive.zip")
    }

    /// Something to call a nameless part — not "attachment", because a
    /// list of four things all called that is a list nobody can pick
    /// from.
    @Test func aNamelessPartIsNamedAfterItsType() {
        let message = """
            Content-Type: multipart/mixed; boundary="b"\r
            \r
            --b\r
            Content-Type: text/plain\r
            \r
            msg\r
            --b\r
            Content-Type: image/jpeg\r
            \r
            JPEG\r
            --b--\r
            """
        let found = MessageAttachments.of(raw(message))
        #expect(found.count == 1)
        #expect(found.first?.filename == "image.jpg")
        #expect(found.first?.inline == false)
    }

    /// Two files may share a name, and a list keyed on the name alone
    /// shows one of them twice.
    @Test func twoFilesWithOneNameAreTwoRows() {
        let message = """
            Content-Type: multipart/mixed; boundary="b"\r
            \r
            --b\r
            Content-Type: application/pdf; name="a.pdf"\r
            \r
            one\r
            --b\r
            Content-Type: application/pdf; name="a.pdf"\r
            \r
            two bytes\r
            --b--\r
            """
        let found = MessageAttachments.of(raw(message))
        #expect(found.count == 2)
        #expect(found[0].id != found[1].id)
    }
}
