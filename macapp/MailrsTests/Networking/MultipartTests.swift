import Testing
import Foundation
@testable import Mailrs

@Suite("MultipartBuilder")
struct MultipartTests {
    @Test("emits text part with name + value")
    func textPart() throws {
        var m = MultipartBuilder(boundary: "B")
        m.appendText(name: "subject", value: "Hi")
        let data = m.finalize()
        let s = try #require(String(data: data, encoding: .utf8))
        #expect(s.contains("--B\r\nContent-Disposition: form-data; name=\"subject\"\r\n\r\nHi\r\n"))
        #expect(s.hasSuffix("--B--\r\n"))
    }

    @Test("emits file part with content-type + binary data")
    func filePart() throws {
        var m = MultipartBuilder(boundary: "X")
        let payload = Data([0x01, 0x02, 0x03, 0xFF])
        m.appendFile(name: "attachments", filename: "bin.dat", contentType: "application/octet-stream", data: payload)
        let data = m.finalize()

        // Header part is UTF-8 text.
        let raw = data as NSData
        let rawString = String(data: data, encoding: .isoLatin1)!
        #expect(rawString.contains("Content-Disposition: form-data; name=\"attachments\"; filename=\"bin.dat\""))
        #expect(rawString.contains("Content-Type: application/octet-stream"))
        // Binary content is preserved literally.
        for byte in [UInt8(0x01), 0x02, 0x03, 0xFF] {
            #expect(data.contains(byte))
        }
        _ = raw  // silence unused warning
    }

    @Test("escapes double quotes in names")
    func escaping() {
        var m = MultipartBuilder(boundary: "Q")
        m.appendText(name: "weird\"name", value: "v")
        let s = String(data: m.finalize(), encoding: .utf8) ?? ""
        #expect(s.contains("name=\"weird\\\"name\""))
    }

    @Test("contentTypeHeader includes boundary")
    func contentTypeHeader() {
        let m = MultipartBuilder(boundary: "X-Y-Z")
        #expect(m.contentTypeHeader == "multipart/form-data; boundary=X-Y-Z")
    }
}
