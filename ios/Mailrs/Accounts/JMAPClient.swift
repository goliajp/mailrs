import Foundation

/// Talking to a JMAP server.
///
/// Ordinary HTTPS, so there is no socket conversation to get wrong —
/// the shapes are in `JMAP`, which needs nothing at all. What is here
/// is the two requests and the seam that lets them be tested: finding
/// the session object, then asking for mail.
actor JMAPClient {
    enum Failure: Error, Equatable {
        case unreachable(String)
        /// The credential was refused — 401, or a session that will
        /// not load.
        case refused(String)
        case server(String)
    }

    private let host: String
    private let http: JMAPHttp

    init(host: String, http: JMAPHttp? = nil) {
        self.host = host
        self.http = http ?? URLSessionHttp()
    }

    /// Find the API url and the account id.
    ///
    /// `/.well-known/jmap` is the only entry point a client may
    /// assume; everything else about a server comes out of what it
    /// answers here.
    func session(user: String, secret: String) async throws -> JMAP.Session {
        let (status, body) = try await exchange(
            url: "https://\(host)/.well-known/jmap",
            authorization: Self.authorization(user: user, secret: secret), body: nil)
        if status == 401 || status == 403 {
            throw Failure.refused("the server refused this account's credential")
        }
        guard (200..<300).contains(status) else {
            throw Failure.server("the server answered \(status)")
        }
        guard let found = JMAP.session(body) else {
            throw Failure.server("the server did not say which account holds the mail")
        }
        return found
    }

    /// The newest messages, in one round trip.
    func newest(
        session: JMAP.Session, user: String, secret: String, limit: Int = 50
    ) async throws -> [JMAP.Email] {
        let (status, body) = try await exchange(
            url: session.apiUrl,
            authorization: Self.authorization(user: user, secret: secret),
            body: JMAP.newestRequest(accountId: session.accountId, limit: limit))
        if status == 401 || status == 403 {
            throw Failure.refused("the server refused this account's credential")
        }
        guard (200..<300).contains(status) else {
            throw Failure.server("the server answered \(status)")
        }
        guard let emails = JMAP.emails(body) else {
            throw Failure.server("the server's answer could not be read")
        }
        return emails
    }

    /// Basic for a password, Bearer for a token.
    ///
    /// Sending a token as a password is refused by every server that
    /// issues tokens — and the person is then told their password is
    /// wrong for an account whose credentials are fine.
    static func authorization(user: String, secret: String) -> String {
        if user.isEmpty { return "Bearer \(secret)" }
        return "Basic \(Data("\(user):\(secret)".utf8).base64EncodedString())"
    }

    private func exchange(url: String, authorization: String, body: String?) async throws -> (
        Int, Data
    ) {
        do {
            return try await http.post(url: url, authorization: authorization, body: body)
        } catch let e as Failure {
            throw e
        } catch {
            throw Failure.unreachable(error.localizedDescription)
        }
    }
}

/// One HTTPS exchange. A seam, so the requests can be tested.
///
/// At file scope because Swift does not allow a protocol inside a
/// type — the nesting that reads better does not compile.
protocol JMAPHttp: Sendable {
    /// - Returns: status and body.
    func post(url: String, authorization: String, body: String?) async throws -> (Int, Data)
}

/// The real one.
struct URLSessionHttp: JMAPHttp {
    func post(url: String, authorization: String, body: String?) async throws -> (Int, Data) {
        guard let target = URL(string: url) else {
            throw JMAPClient.Failure.unreachable("that is not a server address")
        }
        var request = URLRequest(url: target)
        request.httpMethod = "GET"
        if let body {
            request.httpMethod = "POST"
            request.httpBody = Data(body.utf8)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        request.setValue(authorization, forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 30
        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        return (status, data)
    }
}
