import Foundation
import Testing

@testable import Mailrs

/// A Sent-list row has to say when a send did not simply arrive.
struct SendStatusTests {
    @Test func everyStateNeedingAttentionHasAWord() {
        for status in ["failed", "partial", "sending"] {
            #expect(SendStatus.labelKey(status) != nil, "no word for \(status)")
        }
        #expect(SendStatus.labelKey("failed") == "Failed")
    }

    /// No record is not a state: old mail shows nothing rather than a guess.
    @Test func noRecordSaysNothing() {
        #expect(SendStatus.labelKey(nil) == nil)
        #expect(SendStatus.labelKey("something-new") == nil)
    }

    /// The word is translated, so a failed send reads as failed on a
    /// Chinese screen too.
    @Test func everyWordIsInTheCatalog() throws {
        let app = Bundle(for: Session.self)
        let path = try #require(app.path(forResource: "zh-Hans", ofType: "lproj"),
                                "the app shipped no Chinese localization")
        let zh = try #require(Bundle(path: path))
        for status in ["failed", "partial", "sending", "scheduled", "delivered"] {
            let key = try #require(SendStatus.labelKey(status))
            #expect(zh.localizedString(forKey: key, value: key, table: nil) != key,
                    "untranslated: \(key)")
        }
    }
}
