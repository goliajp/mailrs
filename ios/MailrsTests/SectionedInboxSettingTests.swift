import Foundation
import Testing

@testable import Mailrs

/// The sectioned inbox setting's wire shape, the server's
/// `SectionedInbox` (`families/sectioned_inbox.rs`), and its default.
struct SectionedInboxSettingTests {
    @Test func theServersShapeDecodesAndEncodesBack() throws {
        let off = try JSONDecoder().decode(SectionedInboxSetting.self, from: Data(#"{"enabled":false}"#.utf8))
        #expect(off == SectionedInboxSetting(enabled: false))
        let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(off)) as? [String: Bool]
        #expect(encoded == ["enabled": false])
    }

    @Test func onUntilTheReaderTurnsItOff() {
        #expect(SectionedInboxSetting.standard.enabled)
    }
}
