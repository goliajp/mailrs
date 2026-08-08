import Foundation
import Testing

@testable import Mailrs

@MainActor
struct PreferencesTests {
    private func scratch() -> UserDefaults {
        UserDefaults(suiteName: "mailrs-test-\(UUID().uuidString)")!
    }

    /// Reading the preferences must not write them.
    ///
    /// `@Observable` turns a stored property with a `didSet` into an
    /// accessor pair, so the assignments that load these fired the
    /// observer and wrote them back. A UI test that set a language for
    /// one launch had it persisted into every launch after — every
    /// later test ran against a Chinese app, which is how this was
    /// found.
    @Test func loadingDoesNotPersist() {
        let defaults = scratch()
        _ = Preferences(defaults: defaults)
        #expect(defaults.object(forKey: "mailrs.language") == nil)
        #expect(defaults.object(forKey: "mailrs.appearance") == nil)
        #expect(defaults.object(forKey: "mailrs.timeZone") == nil)
    }

    /// Choosing one does write it, and it comes back.
    @Test func choicesSurviveARelaunch() {
        let defaults = scratch()
        let first = Preferences(defaults: defaults)
        first.language = .japanese
        first.appearance = .dark
        first.timeZoneIdentifier = "Asia/Tokyo"

        let second = Preferences(defaults: defaults)
        #expect(second.language == .japanese)
        #expect(second.appearance == .dark)
        #expect(second.timeZone.identifier == "Asia/Tokyo")
    }

    /// Following the system is the absence of an override, so it has
    /// no colour scheme of its own to impose.
    @Test func systemImposesNothing() {
        #expect(Preferences.Appearance.system.colorScheme == nil)
        #expect(Preferences.Language.system.locale == nil)
    }
}
