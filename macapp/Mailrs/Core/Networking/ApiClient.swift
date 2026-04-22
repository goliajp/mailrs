import Foundation

protocol TokenProvider: Sendable {
    func currentToken() async -> String?
    func handleUnauthorized() async
}

actor ApiClient {
    let baseURL: URL
    private let session: URLSession
    private let tokenProvider: TokenProvider

    init(baseURL: URL, session: URLSession = .shared, tokenProvider: TokenProvider) {
        self.baseURL = baseURL
        self.session = session
        self.tokenProvider = tokenProvider
    }

    func get<T: Decodable>(_ path: String, query: [URLQueryItem] = []) async throws -> T {
        try await send(endpoint: Endpoint(path: path, query: query), method: "GET", body: Optional<Data>.none)
    }

    func post<B: Encodable, T: Decodable>(_ path: String, query: [URLQueryItem] = [], body: B) async throws -> T {
        try await send(endpoint: Endpoint(path: path, query: query), method: "POST", body: body)
    }

    func put<B: Encodable, T: Decodable>(_ path: String, query: [URLQueryItem] = [], body: B) async throws -> T {
        try await send(endpoint: Endpoint(path: path, query: query), method: "PUT", body: body)
    }

    func delete<T: Decodable>(_ path: String, query: [URLQueryItem] = []) async throws -> T {
        try await send(endpoint: Endpoint(path: path, query: query), method: "DELETE", body: Optional<Data>.none)
    }

    func postVoid<B: Encodable>(_ path: String, query: [URLQueryItem] = [], body: B) async throws {
        let _: EmptyResponse = try await send(endpoint: Endpoint(path: path, query: query), method: "POST", body: body)
    }

    /// Raw multipart POST. The body is already encoded (no JSON coding) and the Content-Type
    /// header comes from the multipart builder's boundary.
    func postMultipart<T: Decodable>(_ path: String, contentType: String, body: Data) async throws -> T {
        guard let url = Endpoint(path: path).url(relativeTo: baseURL) else {
            throw ApiError.invalidURL
        }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue(contentType, forHTTPHeaderField: "Content-Type")
        req.httpBody = body

        if let token = await tokenProvider.currentToken() {
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: req)
        } catch let urlError as URLError {
            throw ApiError.transport(urlError)
        }

        guard let http = response as? HTTPURLResponse else {
            throw ApiError.malformed("Response is not HTTPURLResponse")
        }

        switch http.statusCode {
        case 200..<300:
            if T.self == EmptyResponse.self { return EmptyResponse() as! T }
            do {
                return try JSONCoders.decoder.decode(T.self, from: data)
            } catch let dec as DecodingError {
                throw ApiError.decoding(dec)
            }
        case 401:
            await tokenProvider.handleUnauthorized()
            throw ApiError.unauthorized
        case 403: throw ApiError.forbidden
        case 404: throw ApiError.notFound
        case 429:
            let retry = http.value(forHTTPHeaderField: "Retry-After").flatMap(TimeInterval.init)
            throw ApiError.rateLimited(retryAfter: retry)
        default:
            let body = try? JSONCoders.decoder.decode(ApiErrorBody.self, from: data)
            throw ApiError.server(status: http.statusCode, message: body?.humanMessage)
        }
    }

    private func send<B: Encodable, T: Decodable>(endpoint: Endpoint, method: String, body: B?) async throws -> T {
        guard let url = endpoint.url(relativeTo: baseURL) else { throw ApiError.invalidURL }

        var req = URLRequest(url: url)
        req.httpMethod = method
        req.setValue("application/json", forHTTPHeaderField: "Accept")

        if let token = await tokenProvider.currentToken() {
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        if let body, !(body is Data) {
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
            req.httpBody = try JSONCoders.encoder.encode(body)
        }

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: req)
        } catch let urlError as URLError {
            throw ApiError.transport(urlError)
        }

        guard let http = response as? HTTPURLResponse else {
            throw ApiError.malformed("Response is not HTTPURLResponse")
        }

        switch http.statusCode {
        case 200..<300:
            if T.self == EmptyResponse.self {
                return EmptyResponse() as! T
            }
            do {
                return try JSONCoders.decoder.decode(T.self, from: data)
            } catch let dec as DecodingError {
                throw ApiError.decoding(dec)
            }
        case 401:
            await tokenProvider.handleUnauthorized()
            throw ApiError.unauthorized
        case 403:
            throw ApiError.forbidden
        case 404:
            throw ApiError.notFound
        case 429:
            let retryAfter = http.value(forHTTPHeaderField: "Retry-After").flatMap(TimeInterval.init)
            throw ApiError.rateLimited(retryAfter: retryAfter)
        default:
            let body = try? JSONCoders.decoder.decode(ApiErrorBody.self, from: data)
            throw ApiError.server(status: http.statusCode, message: body?.humanMessage)
        }
    }
}

struct EmptyResponse: Decodable {}
