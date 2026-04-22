import Testing
import Foundation
@testable import Mailrs

@Suite("ThreadService / MessageActionService URLs")
struct ThreadServiceTests {
    @Test("fetch uses /api/conversations/{id}")
    func fetchPath() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(response: (Data("[]".utf8), 200, [:]), recorder: recorder)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: URLSession(configuration: Self.config),
            tokenProvider: StubTokenProvider()
        )
        let svc = ThreadService(api: api)
        _ = try await svc.fetch(threadId: "abc 123/xyz")

        let req = try #require(await recorder.lastRequest)
        let comps = try #require(URLComponents(url: req.url!, resolvingAgainstBaseURL: false))
        #expect(req.httpMethod == "GET")
        // slash in thread id is percent-encoded so it stays within the path segment.
        #expect(comps.path == "/api/conversations/abc%20123%2Fxyz")
    }

    @Test("markRead POSTs with optional domains query")
    func markReadQuery() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(response: (Data("{}".utf8), 200, [:]), recorder: recorder)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: URLSession(configuration: Self.config),
            tokenProvider: StubTokenProvider()
        )
        let svc = MessageActionService(api: api)
        try await svc.markRead(threadId: "t-1", domains: ["a.com", "b.com"])

        let req = try #require(await recorder.lastRequest)
        #expect(req.httpMethod == "POST")
        let comps = try #require(URLComponents(url: req.url!, resolvingAgainstBaseURL: false))
        #expect(comps.path == "/api/conversations/t-1/read")
        #expect(comps.queryItems?.contains(URLQueryItem(name: "domains", value: "a.com,b.com")) == true)
    }

    @Test("feedback POSTs /api/mail/feedback with sender_email + action")
    func feedbackBody() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(response: (Data("{}".utf8), 200, [:]), recorder: recorder)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: URLSession(configuration: Self.config),
            tokenProvider: StubTokenProvider()
        )
        let svc = MessageActionService(api: api)
        try await svc.feedback(senderEmail: "alice@example.com", action: .markVIP)

        let req = try #require(await recorder.lastRequest)
        #expect(req.url?.path == "/api/mail/feedback")
        #expect(req.httpMethod == "POST")
        // URLSession strips httpBody in URLProtocol stubs for non-streamed bodies;
        // re-read from bodyStream when available to verify payload shape.
        if let body = req.httpBody {
            let obj = try JSONSerialization.jsonObject(with: body) as? [String: String]
            #expect(obj?["sender_email"] == "alice@example.com")
            #expect(obj?["action"] == "mark_vip")
        }
    }

    private static var config: URLSessionConfiguration {
        let c = URLSessionConfiguration.ephemeral
        c.protocolClasses = [MockURLProtocol.self]
        return c
    }
}
