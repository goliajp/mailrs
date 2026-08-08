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

        var label: String {
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

        var label: String {
            switch self {
            case .system: "System"
            case .english: "English"
            case .chinese: "简体中文"
            case .japanese: "日本語"
            }
        }

        var locale: Locale? {
            self == .system ? nil : Locale(identifier: rawValue)
        }
    }

    var appearance: Appearance {
        didSet { defaults.set(appearance.rawValue, forKey: Keys.appearance) }
    }

    var language: Language {
        didSet { defaults.set(language.rawValue, forKey: Keys.language) }
    }

    /// An explicit zone, or `nil` for the phone's. Mail carries
    /// timestamps from everywhere; someone who works across zones needs
    /// to read them all in one.
    var timeZoneIdentifier: String? {
        didSet { defaults.set(timeZoneIdentifier, forKey: Keys.timeZone) }
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

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        appearance = defaults.string(forKey: Keys.appearance)
            .flatMap(Appearance.init(rawValue:)) ?? .system
        language = defaults.string(forKey: Keys.language)
            .flatMap(Language.init(rawValue:)) ?? .system
        timeZoneIdentifier = defaults.string(forKey: Keys.timeZone)
    }
}
