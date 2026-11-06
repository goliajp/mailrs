import Foundation
import Testing

@testable import Mailrs

/// A phone a person removed from another device stays removed: the
/// launch-time registration stops, and only the reader here brings it back.
@MainActor
@Suite struct PushRemovalTests {
    private func fresh() -> UserDefaults {
        let name = "PushRemovalTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return defaults
    }

    @Test func aPhoneNeverRemovedRegistersAsItAlwaysDid() {
        let removal = PushRemoval(defaults: fresh())
        #expect(removal.registration() == false)
        removal.answered(.registered)
        #expect(removal.registration() == false)
        #expect(!removal.removedElsewhere)
    }

    /// Refused once: no more registrations on launch, and it survives a restart.
    @Test func aRemovedPhoneStopsRegisteringOnItsOwn() {
        let defaults = fresh()
        let removal = PushRemoval(defaults: defaults)
        removal.answered(.removed)
        #expect(removal.removedElsewhere)
        #expect(removal.registration() == nil)
        let afterRestart = PushRemoval(defaults: defaults)
        #expect(afterRestart.removedElsewhere)
        #expect(afterRestart.registration() == nil)
    }

    @Test func turningOnAgainRegistersWithResumeOnce() {
        let removal = PushRemoval(defaults: fresh())
        removal.answered(.removed)
        removal.turnOnAgain()
        #expect(removal.registration() == true)
        #expect(removal.removedElsewhere, "still removed until the server takes it back")
        removal.answered(.registered)
        #expect(!removal.removedElsewhere)
        #expect(removal.registration() == false, "later launches do not resume on their own")
    }

    @Test func signingOutForgetsTheRemoval() {
        let removal = PushRemoval(defaults: fresh())
        removal.answered(.removed)
        removal.forget()
        #expect(removal.registration() == false)
    }

    @Test func onlyAGoneWithRemovedIsARemoval() {
        let removed = Data(#"{"error":"removed"}"#.utf8)
        #expect(PushTokenOutcome.isRemoved(status: 410, body: removed))
        #expect(!PushTokenOutcome.isRemoved(status: 410, body: Data(#"{"error":"other"}"#.utf8)))
        #expect(!PushTokenOutcome.isRemoved(status: 204, body: Data()))
        #expect(!PushTokenOutcome.isRemoved(status: 410, body: Data()))
    }

    /// The body has the server's field names: `wire-contract/requests/push-register.json`.
    @Test func theRegistrationBodyHasTheServersFieldNames() throws {
        let body = try JSONEncoder().encode(PushTokenBody(token: "t", platform: "ios", resume: true))
        let keys = try #require(JSONSerialization.jsonObject(with: body) as? [String: Any]).keys
        #expect(Set(keys) == ["token", "platform", "resume"])
    }
}
