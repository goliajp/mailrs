import Testing
import Foundation
@testable import Mailrs

@Suite("MailSendService")
struct MailSendServiceTests {
    @Test("no attachments → POST /api/mail/send with JSON body")
    func jsonPath() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(
            response: (Data(#"{"success": true, "message_id": "abc"}"#.utf8), 200, [:]),
            recorder: recorder
        )
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: URLSession(configuration: Self.config),
            tokenProvider: StubTokenProvider()
        )
        let svc = MailSendService(api: api)

        let payload = SendPayload(
            from: "me@x.com", to: ["a@y.com"], cc: ["b@y.com"],
            subject: "Hi", body: "hello", html_body: "<p>hello</p>"
        )
        let result = try await svc.send(payload)

        let req = try #require(await recorder.lastRequest)
        #expect(req.url?.path == "/api/mail/send")
        #expect(req.httpMethod == "POST")
        #expect(req.value(forHTTPHeaderField: "Content-Type") == "application/json")
        if let body = req.httpBody,
           let json = try JSONSerialization.jsonObject(with: body) as? [String: Any] {
            #expect(json["from"] as? String == "me@x.com")
            #expect((json["to"] as? [String]) == ["a@y.com"])
            #expect((json["cc"] as? [String]) == ["b@y.com"])
            #expect(json["subject"] as? String == "Hi")
            #expect(json["body"] as? String == "hello")
        }
        #expect(result.success)
        #expect(result.message_id == "abc")
    }

    @Test("with attachments → POST /api/mail/send-multipart")
    func multipartPath() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(
            response: (Data(#"{"success": true}"#.utf8), 200, [:]),
            recorder: recorder
        )
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: URLSession(configuration: Self.config),
            tokenProvider: StubTokenProvider()
        )
        let svc = MailSendService(api: api)

        var payload = SendPayload(
            from: "me@x.com", to: ["a@y.com"],
            subject: "With file", body: "body", html_body: "<p>body</p>"
        )
        payload.attachments = [
            AttachmentUpload(filename: "a.txt", contentType: "text/plain", data: Data("hi".utf8))
        ]
        _ = try await svc.send(payload)

        let req = try #require(await recorder.lastRequest)
        #expect(req.url?.path == "/api/mail/send-multipart")
        #expect(req.value(forHTTPHeaderField: "Content-Type")?.hasPrefix("multipart/form-data") == true)
        if let body = req.httpBody, let s = String(data: body, encoding: .utf8) {
            #expect(s.contains("name=\"to\"\r\n\r\na@y.com"))
            #expect(s.contains("name=\"subject\"\r\n\r\nWith file"))
            #expect(s.contains("name=\"attachments\"; filename=\"a.txt\""))
            #expect(s.contains("hi"))
        }
    }

    private static var config: URLSessionConfiguration {
        let c = URLSessionConfiguration.ephemeral
        c.protocolClasses = [MockURLProtocol.self]
        return c
    }
}
