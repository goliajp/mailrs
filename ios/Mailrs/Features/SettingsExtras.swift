import SwiftUI

/// The settings the web keeps under Notifications, Security and Mail, each
/// on a page of its own. Kept out of `SettingsView`, which is the form.
struct SettingsExtras: View {
    @Environment(Preferences.self) private var preferences

    var body: some View {
        @Bindable var preferences = preferences
        Section("Mail and privacy") {
            NavigationLink { NotifySettingsView() } label: {
                Label("Notifications", systemImage: "bell")
            }
            .accessibilityIdentifier("settings.notifications")
            NavigationLink { VacationView() } label: {
                Label("Vacation reply", systemImage: "airplane")
            }
            .accessibilityIdentifier("settings.vacation")
            NavigationLink { MaskedAddressesView() } label: {
                Label("Masked addresses", systemImage: "theatermasks")
            }
            .accessibilityIdentifier("settings.masked")
            Picker("Images in messages", selection: $preferences.remoteImages) {
                Text("Always load").tag(SettingsWire.Images.auto)
                Text("Ask each time").tag(SettingsWire.Images.click)
                Text("Never load").tag(SettingsWire.Images.never)
            }
            .accessibilityIdentifier("settings.images")
        }
        Section("Security") {
            NavigationLink { SessionsView() } label: {
                Label("Signed-in devices", systemImage: "iphone.and.arrow.forward")
            }
            .accessibilityIdentifier("settings.sessions")
            NavigationLink { AppPasswordsView() } label: {
                Label("App passwords", systemImage: "key")
            }
            .accessibilityIdentifier("settings.appPasswords")
        }
    }
}
