import Foundation
import Testing

@testable import Mailrs

/// Asking a JMAP server for a list, and reading what comes back.
@Suite struct JMAPEmailTests {
    /// The back-reference is what makes it one round trip. A client
    /// without it asks, waits, and asks again — two of everything on a
    /// phone, including the latency.
    @Test func theRequestFeedsTheQueryIntoTheGet() {
        let body = JMAP.newestRequest(accountId: "acct-1", limit: 25)
        // Doubled delimiters: the reference itself contains `"#`,
        // which ends an ordinary raw string in the middle of the thing
        // being asserted.
        #expect(body.contains(##""#ids""##))
        #expect(body.contains(#""resultOf":"0""#))
        #expect(body.contains(#""path":"/ids""#))
        #expect(body.contains(#""limit":25"#))
        #expect(body.contains(#""accountId":"acct-1""#))
        // Newest first, or a limit takes the wrong end of the mailbox.
        #expect(body.contains(#""isAscending":false"#))
    }

    private var reply: Data {
        Data(
            #"""
            {"methodResponses":[
              ["Email/query",{"ids":["m1","m2"]},"0"],
              ["Email/get",{"list":[
                {"id":"m1","subject":"Lunch",
                 "from":[{"name":"Ada","email":"ada@example.com"}],
                 "receivedAt":"2025-08-24T01:46:40Z",
                 "keywords":{"$seen":true},
                 "messageId":["<m1@example.com>"]},
                {"id":"m2","subject":"",
                 "from":[{"email":"noreply@example.com"}],
                 "receivedAt":"2025-08-24T02:00:00Z",
                 "keywords":{},
                 "messageId":[]}
              ]},"1"]
            ]}
            """#.utf8)
    }

    /// `from` is a list of objects; reading it as text empties every
    /// row.
    @Test func theSenderIsReadOutOfItsObject() throws {
        let emails = try #require(JMAP.emails(reply))
        #expect(emails[0].sender == "Ada <ada@example.com>")
        // A sender with no display name is its address, not a blank.
        #expect(emails[1].sender == "noreply@example.com")
    }

    /// `keywords` says what is true, so an absent `$seen` means unread
    /// — the same absence IMAP's flag list uses.
    @Test func absenceOfTheSeenKeywordMeansUnread() throws {
        let emails = try #require(JMAP.emails(reply))
        #expect(emails[0].seen)
        #expect(emails[1].seen == false)
    }

    /// `receivedAt` is a UTC date string, not a number.
    @Test func theReceivedTimeIsReadAsUtc() throws {
        let emails = try #require(JMAP.emails(reply))
        #expect(emails[0].receivedAt == 1_756_000_000)
    }

    /// Hand-read rather than handed to a formatter: a formatter brings
    /// a locale and a default time zone with it, which is how a
    /// message moves by hours for somebody who is not in UTC.
    @Test func anUnreadableDateIsNilAndNeverNow() {
        #expect(JMAP.utcDate(nil) == nil)
        #expect(JMAP.utcDate("") == nil)
        #expect(JMAP.utcDate("yesterday") == nil)
        #expect(JMAP.utcDate("2025-08-24 01:46:40Z") == nil)
        #expect(JMAP.utcDate("2025-13-24T01:46:40Z") == nil)
        #expect(JMAP.utcDate("1970-01-01T00:00:00Z") == 0)
    }

    /// A server may answer in any order, and one that puts something
    /// in front of the get shifts it — reading position 1 blindly then
    /// parses the wrong response.
    @Test func theGetIsFoundByNameAndNotByPosition() throws {
        let shifted = Data(
            #"""
            {"methodResponses":[
              ["Core/echo",{},"x"],
              ["Email/query",{"ids":["m1"]},"0"],
              ["Email/get",{"list":[{"id":"m1","subject":"found"}]},"1"]
            ]}
            """#.utf8)
        #expect(try #require(JMAP.emails(shifted)).first?.subject == "found")
    }

    /// Nonsense is nil rather than a crash or an empty list.
    @Test func brokenInputIsNil() {
        #expect(JMAP.emails(Data("not json".utf8)) == nil)
        #expect(JMAP.emails(Data(#"{"methodResponses":[]}"#.utf8)) == nil)
        #expect(JMAP.emails(Data(#"{"methodResponses":[["Email/query",{},"0"]]}"#.utf8)) == nil)
    }
}
