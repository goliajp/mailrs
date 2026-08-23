import Foundation
import Testing

@testable import Mailrs

/// Reading a JMAP session object and a changes reply.
@Suite struct JMAPSessionTests {
    private func data(_ s: String) -> Data { Data(s.utf8) }

    /// `primaryAccounts` is what names the mail account.
    @Test func theMailAccountComesFromPrimaryAccounts() {
        let s = JMAP.session(data("""
            {"apiUrl":"https://api.example.com/jmap",
             "primaryAccounts":{"urn:ietf:params:jmap:mail":"u42"},
             "accounts":{"u1":{},"u42":{}}}
            """))
        #expect(s == JMAP.Session(apiUrl: "https://api.example.com/jmap", accountId: "u42"))
    }

    /// Picking the first key of `accounts` works until somebody has
    /// two — and then it silently reads the wrong mailbox.
    @Test func twoAccountsWithNoPrimaryIsRefusedRatherThanGuessed() {
        #expect(JMAP.session(data("""
            {"apiUrl":"https://api.example.com/jmap","accounts":{"u1":{},"u2":{}}}
            """)) == nil)
    }

    /// One account and no `primaryAccounts` is unambiguous.
    @Test func oneAccountNeedsNoPrimary() {
        #expect(JMAP.session(data("""
            {"apiUrl":"https://api.example.com/jmap","accounts":{"only":{}}}
            """))?.accountId == "only")
    }

    @Test func aSessionWithNoApiUrlIsNotASession() {
        #expect(JMAP.session(data(#"{"accounts":{"u1":{}}}"#)) == nil)
        #expect(JMAP.session(data(#"{"apiUrl":""}"#)) == nil)
        #expect(JMAP.session(data("not json")) == nil)
    }

    @Test func changesCarryTheNewStateAndWhatArrived() {
        let c = JMAP.changes(data("""
            {"methodResponses":[["Email/changes",
              {"created":["m1","m2"],"newState":"s2"},"c0"]]}
            """))
        #expect(c == .some(created: ["m1", "m2"], newState: "s2"))
    }

    /// **Not an error.** RFC 8620 5.2 tells the client to start over;
    /// treating it as a failure leaves an account that never syncs
    /// again.
    @Test func cannotCalculateChangesMeansStartOverNotFail() {
        #expect(JMAP.changes(data("""
            {"methodResponses":[["error",{"type":"cannotCalculateChanges"},"c0"]]}
            """)) == .startOver)
    }

    /// Any other error is a failure and must not be read as a fresh
    /// start — that would silently re-download the mailbox.
    @Test func anotherErrorIsNotAFreshStart() {
        #expect(JMAP.changes(data("""
            {"methodResponses":[["error",{"type":"accountNotFound"},"c0"]]}
            """)) == nil)
    }

    @Test func aReplyWithNoStateIsNotUsable() {
        #expect(JMAP.changes(data(#"{"methodResponses":[["Email/changes",{},"c0"]]}"#)) == nil)
        #expect(JMAP.changes(data(#"{"methodResponses":[]}"#)) == nil)
    }
}
