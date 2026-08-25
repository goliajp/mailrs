import SwiftUI

/// Preferences, in the window this platform keeps them in.
///
/// A Mac app without a Settings scene has a greyed-out ⌘, and puts its
/// options behind a button somewhere in the content — which is the
/// phone's answer, because a phone has no second window to put them
/// in. Here they get one, laid out as a form with labels on the left,
/// which is what every other Preferences window on the machine looks
/// like.
///
/// The same three the phone offers, and no more: this is not the place
/// to grow options the other platforms do not have, or the two builds
/// start disagreeing about what the app can do.
struct MacSettingsView: View {
    @Environment(Preferences.self) private var preferences

    var body: some View {
        @Bindable var preferences = preferences
        Form {
            Picker("Appearance", selection: $preferences.appearance) {
                ForEach(Preferences.Appearance.allCases) { option in
                    Text(option.label).tag(option)
                }
            }
            .accessibilityIdentifier("mac.settings.appearance")

            Picker("Language", selection: $preferences.language) {
                ForEach(Preferences.Language.allCases) { option in
                    Text(option.label).tag(option)
                }
            }
            .accessibilityIdentifier("mac.settings.language")
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .padding()
        .accessibilityIdentifier("mac.settings")
    }
}
