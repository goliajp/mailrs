import Foundation

/// Text resolved **here**, against the language this app is set to.
///
/// SwiftUI looks a `LocalizedStringKey` up in the `\.locale` the app
/// overrides for its in-app language picker. Nothing else does:
/// `String(localized:)` reads the table of the *process* language, which
/// is the phone's, so a string built in code — a banner, a guest count,
/// an error — stayed English on a phone set to English with the app set
/// to Japanese. Strings built in code go through `AppText`.
///
/// **The `locale:` parameter is not what picks the language.** It
/// governs formatting — how a number or a date is written — and the
/// table a key is looked up in comes from the *bundle*, so the bundle
/// of the chosen language is found first.
enum AppText {
    /// In the language chosen in Settings, or the phone's.
    static func string(_ value: String.LocalizationValue) -> String {
        string(value, in: Preferences.chosenLocale() ?? Locale.autoupdatingCurrent)
    }

    static func string(_ value: String.LocalizationValue, in locale: Locale) -> String {
        // Apple's own matching, so `zh_CN` finds `zh-Hans.lproj`
        let match = Bundle.preferredLocalizations(
            from: Bundle.main.localizations, forPreferences: [locale.identifier]
        ).first
        guard let match,
            let path = Bundle.main.path(forResource: match, ofType: "lproj"),
            let bundle = Bundle(path: path)
        else {
            return String(localized: value, locale: locale)
        }
        return String(localized: value, bundle: bundle, locale: locale)
    }
}

/// A title resolved in code rather than by SwiftUI.
///
/// A `navigationTitle` taking a `LocalizedStringKey` is hoisted into
/// the window's title bar and resolved there, outside the `\.locale`
/// override, and one window then shows two languages: an English source
/// list beside a Chinese column header, which is what the Mac build did.
/// The `String` overload of `navigationTitle` does not localise, which
/// is what is wanted once the string is already the right one.
enum LocalisedTitle {
    static func of(_ key: String, in locale: Locale) -> String {
        AppText.string(String.LocalizationValue(key), in: locale)
    }
}
