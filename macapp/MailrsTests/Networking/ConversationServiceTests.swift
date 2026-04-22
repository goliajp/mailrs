import Testing
import Foundation
@testable import Mailrs

@Suite("ConversationService URL building")
struct ConversationServiceTests {
    @Test("list with defaults sends limit only")
    func defaultList() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(
            response: (Data("[]".utf8), 200, [:]),
            recorder: recorder
        )
        let session = URLSession(configuration: Self.sessionConfig)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: session,
            tokenProvider: StubTokenProvider()
        )
        let svc = ConversationService(api: api)

        _ = try await svc.list(ConversationListOptions())

        let req = try #require(await recorder.lastRequest)
        let comps = try #require(URLComponents(url: req.url!, resolvingAgainstBaseURL: false))
        #expect(comps.path == "/api/conversations")
        #expect(comps.queryItems?.contains(URLQueryItem(name: "limit", value: "50")) == true)
        #expect(comps.queryItems?.contains { $0.name == "folder" } == false)
    }

    @Test("list with filters includes all expected params")
    func fullFilters() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(
            response: (Data("[]".utf8), 200, [:]),
            recorder: recorder
        )
        let session = URLSession(configuration: Self.sessionConfig)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: session,
            tokenProvider: StubTokenProvider()
        )
        let svc = ConversationService(api: api)

        let opts = ConversationListOptions(
            limit: 30,
            before: 1700000000,
            category: .promotion,
            folder: .sent,
            quickFilter: .unread,
            domains: ["a.com", "b.com"],
            archived: true,
            section: "important"
        )
        _ = try await svc.list(opts)

        let req = try #require(await recorder.lastRequest)
        let comps = try #require(URLComponents(url: req.url!, resolvingAgainstBaseURL: false))
        let items = comps.queryItems ?? []
        #expect(items.contains(URLQueryItem(name: "limit", value: "30")))
        #expect(items.contains(URLQueryItem(name: "before", value: "1700000000")))
        #expect(items.contains(URLQueryItem(name: "category", value: "promotion")))
        #expect(items.contains(URLQueryItem(name: "folder", value: "Sent")))
        #expect(items.contains(URLQueryItem(name: "domains", value: "a.com,b.com")))
        #expect(items.contains(URLQueryItem(name: "archived", value: "true")))
        #expect(items.contains(URLQueryItem(name: "unread", value: "true")))
        #expect(items.contains(URLQueryItem(name: "section", value: "important")))
    }

    @Test("search hits /search endpoint with q param")
    func searchEndpoint() async throws {
        let recorder = RequestRecorder()
        MockURLProtocol.register(
            response: (Data("[]".utf8), 200, [:]),
            recorder: recorder
        )
        let session = URLSession(configuration: Self.sessionConfig)
        let api = ApiClient(
            baseURL: URL(string: "http://localhost:3200")!,
            session: session,
            tokenProvider: StubTokenProvider()
        )
        let svc = ConversationService(api: api)

        _ = try await svc.search("invoice Q2", options: ConversationListOptions())

        let req = try #require(await recorder.lastRequest)
        let comps = try #require(URLComponents(url: req.url!, resolvingAgainstBaseURL: false))
        #expect(comps.path == "/api/conversations/search")
        let items = comps.queryItems ?? []
        #expect(items.contains(URLQueryItem(name: "q", value: "invoice Q2")))
        #expect(items.contains(URLQueryItem(name: "limit", value: "50")))
    }

    private static var sessionConfig: URLSessionConfiguration {
        let c = URLSessionConfiguration.ephemeral
        c.protocolClasses = [MockURLProtocol.self]
        return c
    }
}
