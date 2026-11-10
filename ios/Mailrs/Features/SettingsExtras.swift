import SwiftUI

/// The settings the web keeps under Notifications, Security and Mail, each
/// on a page of its own. Kept out of `SettingsView`, which is the form.
struct SettingsExtras: View {
    @Environment(Preferences.self) private var preferences

    var body: some View {
        @Bindable var preferences = preferences
        Section("Mail and privacy") {
            NavigationLink { NotifySettingsView() } label: {
                SettingsRowLabel("Notifications", systemImage: "bell")
            }
            .accessibilityIdentifier("settings.notifications")
            NavigationLink { VacationView() } label: {
                SettingsRowLabel("Vacation responder", systemImage: "airplane")
            }
            .accessibilityIdentifier("settings.vacation")
            NavigationLink { MaskedAddressesView() } label: {
                SettingsRowLabel("Masked addresses", systemImage: "theatermasks")
            }
            .accessibilityIdentifier("settings.masked")
            Picker("Images in messages", selection: $preferences.remoteImages) {
                Text("Always load").tag(SettingsWire.Images.auto)
                Text("Ask each time").tag(SettingsWire.Images.click)
                Text("Never load").tag(SettingsWire.Images.never)
            }
            .accessibilityIdentifier("settings.images")
        }
        SectionedInboxSettingsSection()
        SwipeSettingsSection()
        Section("Security") {
            NavigationLink { SessionsView() } label: {
                SettingsRowLabel("Signed-in devices", systemImage: "iphone.and.arrow.forward")
            }
            .accessibilityIdentifier("settings.sessions")
            NavigationLink { AppPasswordsView() } label: {
                SettingsRowLabel("App passwords", systemImage: "key")
            }
            .accessibilityIdentifier("settings.appPasswords")
        }
    }
}

/// A settings row's title with its icon in the accent colour, as the
/// account rows above draw theirs; a plain `Label` drew these icons
/// black, and the masks in two colours.
struct SettingsRowLabel: View {
    let title: LocalizedStringKey
    let systemImage: String

    init(_ title: LocalizedStringKey, systemImage: String) {
        self.title = title
        self.systemImage = systemImage
    }

    var body: some View {
        Label {
            Text(title)
        } icon: {
            Image(systemName: systemImage)
                .symbolRenderingMode(.monochrome)
                .foregroundStyle(Color.accentColor)
        }
    }
}
