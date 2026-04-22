import Testing
import Foundation
@testable import Mailrs

@Suite("ApiClient")
struct ApiClientTests {
    @Test("injects bearer token on every request")
    func bearerInjection() async throws {
        let recorder = RequestRecorder()
        let (session, tokenProvider) = Self.makeSession(
            recorder: recorder,
            response: (Data("{}".utf8), 200)
        )
        tokenProvider.tokenOverride = "hexhexhex"

        let client = ApiClient(baseURL: URL(string: "http://localhost:3200")!, session: session, tokenProvider: tokenProvider)
        let _: EmptyResponse = try await client.get("/api/foo")

        let request = try #require(await recorder.lastRequest)
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer hexhexhex")
        #expect(request.url?.path == "/api/foo")
    }

    @Test("401 triggers handleUnauthorized and throws")
    func unauthorizedHook() async throws {
        let recorder = RequestRecorder()
        let (session, tokenProvider) = Self.makeSession(
            recorder: recorder,
            response: (Data("{}".utf8), 401)
        )
        tokenProvider.tokenOverride = "whatever"

        let client = ApiClient(baseURL: URL(string: "http://localhost:3200")!, session: session, tokenProvider: tokenProvider)

        await #expect(throws: ApiError.self) {
            let _: EmptyResponse = try await client.get("/api/bar")
        }
        #expect(tokenProvider.unauthorizedCallCount == 1)
    }

    @Test("429 surfaces rate limit with Retry-After")
    func rateLimit() async throws {
        let recorder = RequestRecorder()
        let (session, tokenProvider) = Self.makeSession(
            recorder: recorder,
            response: (Data("{}".utf8), 429, ["Retry-After": "30"])
        )

        let client = ApiClient(baseURL: URL(string: "http://localhost:3200")!, session: session, tokenProvider: tokenProvider)

        do {
            let _: EmptyResponse = try await client.get("/api/baz")
            Issue.record("expected throw")
        } catch ApiError.rateLimited(let retryAfter) {
            #expect(retryAfter == 30)
        }
    }

    @Test("builds query items into URL")
    func queryItems() async throws {
        let recorder = RequestRecorder()
        let (session, tokenProvider) = Self.makeSession(
            recorder: recorder,
            response: (Data("{}".utf8), 200)
        )
        let client = ApiClient(baseURL: URL(string: "http://localhost:3200")!, session: session, tokenProvider: tokenProvider)

        let _: EmptyResponse = try await client.get("/api/conversations", query: [
            URLQueryItem(name: "limit", value: "50"),
            URLQueryItem(name: "folder", value: "INBOX"),
        ])

        let request = try #require(await recorder.lastRequest)
        let comps = try #require(URLComponents(url: request.url!, resolvingAgainstBaseURL: false))
        #expect(comps.queryItems?.contains(URLQueryItem(name: "limit", value: "50")) == true)
        #expect(comps.queryItems?.contains(URLQueryItem(name: "folder", value: "INBOX")) == true)
    }

    // MARK: helpers

    private static func makeSession(
        recorder: RequestRecorder,
        response: (data: Data, status: Int, headers: [String: String])
    ) -> (URLSession, StubTokenProvider) {
        MockURLProtocol.register(response: response, recorder: recorder)
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)
        return (session, StubTokenProvider())
    }

    private static func makeSession(
        recorder: RequestRecorder,
        response: (data: Data, status: Int)
    ) -> (URLSession, StubTokenProvider) {
        makeSession(recorder: recorder, response: (response.data, response.status, [:]))
    }
}

final class StubTokenProvider: TokenProvider, @unchecked Sendable {
    var tokenOverride: String?
    var unauthorizedCallCount: Int = 0

    func currentToken() async -> String? { tokenOverride }
    func handleUnauthorized() async { unauthorizedCallCount += 1 }
}

actor RequestRecorder {
    private(set) var lastRequest: URLRequest?
    private(set) var allRequests: [URLRequest] = []

    func record(_ request: URLRequest) {
        lastRequest = request
        allRequests.append(request)
    }
}

final class MockURLProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var stubResponse: (data: Data, status: Int, headers: [String: String])?
    nonisolated(unsafe) static var recorder: RequestRecorder?

    static func register(
        response: (data: Data, status: Int, headers: [String: String]),
        recorder: RequestRecorder
    ) {
        stubResponse = response
        self.recorder = recorder
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        if let recorder = Self.recorder {
            let captured = request
            Task { await recorder.record(captured) }
        }
        guard let stub = Self.stubResponse else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }
        let httpResponse = HTTPURLResponse(
            url: request.url!,
            statusCode: stub.status,
            httpVersion: "HTTP/1.1",
            headerFields: stub.headers
        )!
        client?.urlProtocol(self, didReceive: httpResponse, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: stub.data)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
