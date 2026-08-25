import Foundation

/// A title resolved **here**, against the language this app is set to.
///
/// A `navigationTitle` taking a `LocalizedStringKey` is hoisted into
/// the window's title bar and resolved there, outside the `\.locale`
/// this app overrides for its in-app language picker. The result is
/// one window showing two languages: an English source list beside a
/// Chinese column header, which is what the Mac build did.
///
/// Resolving to a `String` in code settles it before SwiftUI decides
/// where to draw it. The `String` overload of `navigationTitle` does
/// not localise, which is exactly what is wanted once the string is
/// already the right one.
/// **The `locale:` parameter is not what picks the language.** It
/// governs formatting — how a number or a date is written — and the
/// table a key is looked up in comes from the *bundle*. Passing a
/// locale and expecting Chinese to become English is a mistake that
/// looks like it should work, and the first version of this made it.
enum LocalisedTitle {
    static func of(_ key: String, in locale: Locale) -> String {
        let value = String.LocalizationValue(key)
        guard let code = locale.language.languageCode?.identifier,
            let path = Bundle.main.path(forResource: code, ofType: "lproj"),
            let bundle = Bundle(path: path)
        else {
            return String(localized: value, locale: locale)
        }
        return String(localized: value, bundle: bundle, locale: locale)
    }
}
