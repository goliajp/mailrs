import Foundation
import SwiftUI

/// What the reader has chosen about how the app looks and reads.
///
/// Stored in `UserDefaults` rather than the Keychain: none of it is a
/// credential, and it should survive a sign-out — signing back in
/// should not hand you someone else's idea of dark mode.
@Observable
@MainActor
final class Preferences {
    enum Appearance: String, CaseIterable, Identifiable, Sendable {
        case system, light, dark
        var id: String { rawValue }

        var label: LocalizedStringKey {
            switch self {
            case .system: "System"
            case .light: "Light"
            case .dark: "Dark"
            }
        }

        /// `nil` means "whatever the phone says" — SwiftUI's own way of
        /// spelling it, so following the system is the absence of an
        /// override rather than a value that has to track it.
        var colorScheme: ColorScheme? {
            switch self {
            case .system: nil
            case .light: .light
            case .dark: .dark
            }
        }
    }

    /// The languages the app is offered in. `system` follows the
    /// phone's own order, which is what most people want and what iOS
    /// does by default.
    enum Language: String, CaseIterable, Identifiable, Sendable {
        case system
        case english = "en"
        case chinese = "zh-Hans"
        case japanese = "ja"

        var id: String { rawValue }

        var label: LocalizedStringKey {
            switch self {
            case .system: "System"
            case .english: "English"
            case .chinese: "简体中文"
            case .japanese: "日本語"
            }
        }

        var locale: Locale? {
            guard self != .system else { return nil }
            return Locale(identifier: rawValue)
        }
    }

    var appearance: Appearance {
        didSet { persist(appearance.rawValue, Keys.appearance) }
    }

    var language: Language {
        didSet { persist(language.rawValue, Keys.language) }
    }

    /// An explicit zone, or `nil` for the phone's. Mail carries
    /// timestamps from everywhere; someone who works across zones needs
    /// to read them all in one.
    var timeZoneIdentifier: String? {
        didSet { persist(timeZoneIdentifier, Keys.timeZone) }
    }

    var timeZone: TimeZone {
        timeZoneIdentifier.flatMap(TimeZone.init(identifier:)) ?? .autoupdatingCurrent
    }

    private enum Keys {
        static let appearance = "mailrs.appearance"
        static let language = "mailrs.language"
        static let timeZone = "mailrs.timeZone"
    }

    private let defaults: UserDefaults

    /// `@Observable` rewrites a stored property with a `didSet` into an
    /// accessor pair, and the assignments in `init` go through it — so
    /// loading these wrote them straight back. Harmless for a value the
    /// reader chose, and not at all harmless for one that arrived in the
    /// launch arguments: a UI test that set a language for one launch
    /// found it persisted into every launch after it.
    private var loaded = false

    private func persist(_ value: String?, _ key: String) {
        guard loaded else { return }
        defaults.set(value, forKey: key)
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        appearance = defaults.string(forKey: Keys.appearance)
            .flatMap(Appearance.init(rawValue:)) ?? .system
        language = defaults.string(forKey: Keys.language)
            .flatMap(Language.init(rawValue:)) ?? .system
        timeZoneIdentifier = defaults.string(forKey: Keys.timeZone)
        loaded = true
    }
}
