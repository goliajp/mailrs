import Foundation
import Testing

@testable import Mailrs

/// The shape `GET /api/accounts/external` actually answers.
///
/// It is an **object with one key**, not a bare array — and this client
/// decoded it as an array for as long as the screen existed. The list
/// came back empty and the failure was swallowed, so it read as "you
/// have not connected anything". Nothing caught it because the shared
/// stub did not serve the route at all.
@Suite struct ExternalAccountWireTests {
    /// Captured from `crates/webapi/src/handlers/external_accounts.rs::list`.
    private let body = Data("""
        {"accounts":[
          {"id":"acc_gmail","email":"someone@gmail.com","display_name":"Work",
           "provider":"gmail","colour":"#4285f4","state":"ok","auth":"oauth2",
           "last_sync":1754400000,"progress":null,"last_error":null}
        ]}
        """.utf8)

    @Test func theListIsAnObjectWithOneKey() throws {
        let out = try JSONDecoder().decode(Wire.ExternalAccountList.self, from: body)
        #expect(out.accounts.count == 1)
        #expect(out.accounts.first?.email == "someone@gmail.com")
    }

    /// The mistake, stated as a fact: reading this body as an array
    /// throws. What guards the client's own choice is
    /// `AccountFlowTests`, against a stub that now serves the route.
    @Test func theBodyIsNotAnArray() {
        #expect(throws: (any Error).self) {
            try JSONDecoder().decode([Wire.ExternalAccount].self, from: body)
        }
    }

    @Test func anEmptyAnswerIsAListWithNothingInIt() throws {
        let out = try JSONDecoder().decode(
            Wire.ExternalAccountList.self, from: Data(#"{"accounts":[]}"#.utf8))
        #expect(out.accounts.isEmpty)
    }

    /// A row written before a field existed must still decode: the
    /// alternative is a screen that goes blank on an old row.
    @Test func aRowFromBeforeProgressExistedStillDecodes() throws {
        let out = try JSONDecoder().decode(
            Wire.ExternalAccountList.self,
            from: Data(#"{"accounts":[{"id":"a","email":"x@y.jp","state":"ok"}]}"#.utf8))
        #expect(out.accounts.first?.progress == nil)
        #expect(out.accounts.first?.state == "ok")
    }

    /// What the sync worker writes while a full re-read is running.
    @Test func whatItIsDoingRightNowSurvivesTheWire() throws {
        let out = try JSONDecoder().decode(
            Wire.ExternalAccountList.self,
            from: Data(#"""
                {"accounts":[{"id":"a","email":"x@y.jp","state":"ok",
                 "progress":"reading Inbox again from the start"}]}
                """#.utf8))
        #expect(out.accounts.first?.progress == "reading Inbox again from the start")
    }
}
