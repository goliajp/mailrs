import Foundation
import Testing

@testable import Mailrs

struct MultipartFormTests {
    @Test func laysOutFieldsAndFilesToTheByte() {
        let body = MultipartForm.encode(
            fields: [("to", "a@b.com"), ("subject", "Hi")],
            files: [.init(name: "attachments", filename: "x.txt",
                          contentType: "text/plain", data: Data("hello".utf8))],
            boundary: "B"
        )
        let text = String(decoding: body, as: UTF8.self)
        #expect(text == """
        --B\r
        Content-Disposition: form-data; name="to"\r
        \r
        a@b.com\r
        --B\r
        Content-Disposition: form-data; name="subject"\r
        \r
        Hi\r
        --B\r
        Content-Disposition: form-data; name="attachments"; filename="x.txt"\r
        Content-Type: text/plain\r
        \r
        hello\r
        --B--\r

        """)
    }

    /// Binary survives untouched — the file rides between CRLFs, not
    /// through any string round-trip.
    @Test func binaryDataIsNotMangled() {
        let bytes = Data([0x00, 0xFF, 0x0D, 0x0A, 0x89])
        let body = MultipartForm.encode(
            fields: [],
            files: [.init(name: "attachments", filename: "b.bin",
                          contentType: "application/octet-stream", data: bytes)],
            boundary: "B"
        )
        #expect(body.range(of: bytes) != nil)
    }

    /// A quote or newline in a filename must not splice the header.
    @Test func hostileFilenamesAreDefused() {
        let body = MultipartForm.encode(
            fields: [],
            files: [.init(name: "attachments", filename: "a\"; name=\"evil\r\n.txt",
                          contentType: "text/plain", data: Data())],
            boundary: "B"
        )
        let text = String(decoding: body, as: UTF8.self)
        #expect(!text.contains("name=\"evil"))
    }
}
