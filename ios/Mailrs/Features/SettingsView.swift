import SwiftUI

/// One place for the choices that were scattered across the app: the
/// appearance, how dates read, which server, and who is signed in.
///
/// Before this, the server lived only on the sign-in form and sign-out
/// hid in the list menu — findable if you already knew, which is not
/// the same as findable.
struct SettingsView: View {
    @Environment(Session.self) private var session
    @Environment(Preferences.self) private var preferences
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    @State private var showingAliases = false
    @State private var showingAccounts = false
    @State private var showingDomains = false

    /// The zones worth offering by name, plus whatever the phone is
    /// set to. A full tz database picker is a list of 400 strings and
    /// the answer is almost always one of these.
    private var zoneChoices: [String] {
        let common = ["Asia/Tokyo", "Asia/Shanghai", "Asia/Singapore",
                      "Europe/London", "Europe/Berlin",
                      "America/New_York", "America/Los_Angeles", "UTC"]
        let current = TimeZone.autoupdatingCurrent.identifier
        return common.contains(current) ? common : [current] + common
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Account") {
                    LabeledContent("Signed in as", value: session.myAddress)
                    LabeledContent("Server", value: session.baseURL.host() ?? "—")
                }

                Section("Appearance") {
                    Picker("Theme", selection: appearanceBinding) {
                        ForEach(Preferences.Appearance.allCases) { option in
                            Text(option.label).tag(option)
                        }
                    }
                    .pickerStyle(.segmented)
                }

                Section {
                    Picker("Language", selection: languageBinding) {
                        ForEach(Preferences.Language.allCases) { option in
                            Text(option.label).tag(option)
                        }
                    }
                    Picker("Time zone", selection: zoneBinding) {
                        Text("System").tag(String?.none)
                        ForEach(zoneChoices, id: \.self) { zone in
                            Text(zone.replacingOccurrences(of: "_", with: " "))
                                .tag(String?.some(zone))
                        }
                    }
                } header: {
                    Text("Language and time")
                } footer: {
                    // Said plainly, because the effect is otherwise
                    // invisible until a message from another zone
                    // arrives.
                    Text("Times on messages are shown in this zone.")
                }

                Section("Administration") {
                    Button {
                        showingAccounts = true
                    } label: {
                        Label("Accounts", systemImage: "person.2")
                    }
                    Button {
                        showingAliases = true
                    } label: {
                        Label("Aliases", systemImage: "arrow.triangle.branch")
                    }
                    Button {
                        showingDomains = true
                    } label: {
                        Label("Domains", systemImage: "globe")
                    }
                }

                Section {
                    Button(role: .destructive) {
                        session.signOut()
                        dismiss()
                    } label: {
                        Text("Sign out").frame(maxWidth: .infinity)
                    }
                }
            }
            .sheet(isPresented: $showingAliases) { AliasesView() }
            .sheet(isPresented: $showingAccounts) { AccountsView() }
            .sheet(isPresented: $showingDomains) { DomainsView() }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private var appearanceBinding: Binding<Preferences.Appearance> {
        Binding(get: { preferences.appearance }, set: { preferences.appearance = $0 })
    }

    private var languageBinding: Binding<Preferences.Language> {
        Binding(get: { preferences.language }, set: { preferences.language = $0 })
    }

    private var zoneBinding: Binding<String?> {
        Binding(get: { preferences.timeZoneIdentifier },
                set: { preferences.timeZoneIdentifier = $0 })
    }
}
