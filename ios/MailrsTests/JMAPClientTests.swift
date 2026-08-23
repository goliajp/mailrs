import Foundation
import Testing

@testable import Mailrs

/// A server that answers what it is told to answer.
actor FakeHttp: JMAPHttp {
    private var answers: [(Int, String)]
    private(set) var asked: [(url: String, authorization: String, body: String?)] = []

    init(_ answers: [(Int, String)]) { self.answers = answers }

    func post(url: String, authorization: String, body: String?) async throws -> (Int, Data) {
        asked.append((url, authorization, body))
        guard !answers.isEmpty else { return (500, Data()) }
        let next = answers.removeFirst()
        return (next.0, Data(next.1.utf8))
    }
}

/// Asking a JMAP server, without one.
@Suite struct JMAPClientTests {
    private func client(_ answers: [(Int, String)]) -> (JMAPClient, FakeHttp) {
        let fake = FakeHttp(answers)
        return (JMAPClient(host: "mail.example.com", http: fake), fake)
    }

    /// Sending a token as a password is refused by every server that
    /// issues tokens — and the person is then told their password is
    /// wrong for an account whose credentials are fine.
    @Test func aTokenIsABearerAndAPasswordIsBasic() {
        #expect(JMAPClient.authorization(user: "", secret: "tok-123") == "Bearer tok-123")
        let basic = JMAPClient.authorization(user: "me@example.com", secret: "secret")
        #expect(basic.hasPrefix("Basic "))
        let payload = String(basic.dropFirst("Basic ".count))
        let decoded = String(decoding: Data(base64Encoded: payload) ?? Data(), as: UTF8.self)
        #expect(decoded == "me@example.com:secret")
    }

    /// `/.well-known/jmap` is the only entry point a client may assume.
    @Test func theSessionIsAskedForAtTheWellKnownPlace() async throws {
        let body = """
            {"apiUrl":"https://api.example.com/jmap",
             "primaryAccounts":{"urn:ietf:params:jmap:mail":"acct-9"}}
            """
        let (c, http) = client([(200, body)])
        let found = try await c.session(user: "me@example.com", secret: "secret")
        let asked = await http.asked
        #expect(asked.first?.url == "https://mail.example.com/.well-known/jmap")
        #expect(found.accountId == "acct-9")
        #expect(found.apiUrl == "https://api.example.com/jmap")
        // A GET, not a POST: nothing is being sent.
        #expect(asked.first?.body == nil)
    }

    /// A refused credential is a refusal, not a server fault — the two
    /// lead a person to do completely different things.
    @Test func a401IsARefusedCredential() async throws {
        let (c, _) = client([(401, #"{"type":"unauthorized"}"#)])
        await #expect(throws: JMAPClient.Failure.self) {
            _ = try await c.session(user: "me@example.com", secret: "wrong")
        }
    }

    /// The mail request goes to the api url the session named.
    @Test func mailIsAskedForAtTheApiUrl() async throws {
        let reply = """
            {"methodResponses":[["Email/get",{"list":[
              {"id":"m1","subject":"hi","from":[{"email":"a@b.com"}],
               "receivedAt":"2025-08-24T01:46:40Z","keywords":{}}]},"1"]]}
            """
        let (c, http) = client([(200, reply)])
        let found = JMAP.Session(apiUrl: "https://api.example.com/jmap", accountId: "acct-9")
        let emails = try await c.newest(
            session: found, user: "me@example.com", secret: "secret", limit: 10)
        let asked = await http.asked
        #expect(asked.first?.url == "https://api.example.com/jmap")
        #expect(asked.first?.body?.contains(#""accountId":"acct-9""#) == true)
        #expect(emails.count == 1)
        #expect(emails.first?.subject == "hi")
    }

    /// A session object that does not say which account holds the mail
    /// is not a session — guessing there reads somebody else's mailbox.
    @Test func anAmbiguousSessionIsRefusedRatherThanGuessed() async throws {
        let body = #"{"apiUrl":"https://api.example.com/jmap","accounts":{"a":{},"b":{}}}"#
        let (c, _) = client([(200, body)])
        await #expect(throws: JMAPClient.Failure.self) {
            _ = try await c.session(user: "me@example.com", secret: "secret")
        }
    }
}
