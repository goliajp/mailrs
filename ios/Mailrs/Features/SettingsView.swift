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
    @State private var showingGroups = false
    @State private var showingQueue = false
    @State private var showingKeys = false
    @State private var showingDmarc = false
    @State private var showingAudit = false
    @State private var showingPermissions = false

    /// The zones worth offering by name, plus whatever the phone is
    /// set to. A full tz database picker is a list of 400 strings and
    /// the answer is almost always one of these.
    private var zoneChoices: [String] {
        let common = ["Asia/Tokyo", "Asia/Shanghai", "Asia/Singapore",
                      "Europe/London", "Europe/Berlin",
                      "America/New_York", "America/Los_Angeles", "UTC"]
        let current = TimeZone.autoupdatingCurrent.identifier
        if common.contains(current) { return common }
        return [current] + common
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Account") {
                    LabeledContent("Signed in as", value: session.myAddress)
                    LabeledContent("Server", value: session.baseURL.host() ?? "—")
                    // Not under Administration: a key acts as *this*
                    // account, and revoking one is the account holder's
                    // business rather than an operator's.
                    Button {
                        showingKeys = true
                    } label: {
                        LucideRow(title: "API keys", icon: Lucide.keyRound)
                    }
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

                // Only where it can be honoured. A device with no
                // passcode has nothing to authenticate against, and a
                // switch that silently does nothing is worse than none.
                if BiometricLock.isAvailable {
                    Section {
                        Toggle(isOn: lockBinding) {
                            LucideRow(title: BiometricLock.kind().label,
                                      icon: BiometricLock.kind().symbol)
                        }
                        .accessibilityIdentifier("require-biometrics")
                    } header: {
                        Text("Privacy")
                    } footer: {
                        Text("Asked for when the app starts, and when you come back to it after a minute away.")
                    }
                }

                // Two sections, not one of seven: directory work and
                // operational work are different errands, and a list
                // long enough to scroll should say where it changes
                // subject.
                Section("Directory") {
                    Button {
                        showingAccounts = true
                    } label: {
                        LucideRow(title: "Accounts", icon: Lucide.users)
                    }
                    Button {
                        showingAliases = true
                    } label: {
                        LucideRow(title: "Aliases", icon: Lucide.split)
                    }
                    Button {
                        showingGroups = true
                    } label: {
                        LucideRow(title: "Groups", icon: Lucide.mails)
                    }
                    Button {
                        showingDomains = true
                    } label: {
                        LucideRow(title: "Domains", icon: Lucide.globe)
                    }
                    Button {
                        showingPermissions = true
                    } label: {
                        LucideRow(title: "Permissions", icon: Lucide.lockKeyhole)
                    }
                }

                Section("Operations") {
                    Button {
                        showingQueue = true
                    } label: {
                        LucideRow(title: "Queue", icon: Lucide.send)
                    }
                    Button {
                        showingDmarc = true
                    } label: {
                        LucideRow(title: "DMARC", icon: Lucide.shieldCheck)
                    }
                    Button {
                        showingAudit = true
                    } label: {
                        LucideRow(title: "Audit log", icon: Lucide.scrollText)
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
            .sheet(isPresented: $showingGroups) { EmailGroupsView() }
            .sheet(isPresented: $showingQueue) { QueueView() }
            .sheet(isPresented: $showingKeys) { AgentKeysView() }
            .sheet(isPresented: $showingDmarc) { DmarcView() }
            .sheet(isPresented: $showingAudit) { AuditLogView() }
            .sheet(isPresented: $showingPermissions) { PermissionGroupsView() }
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

    /// Turning it on asks once, straight away.
    ///
    /// Otherwise the first time it is exercised is a cold launch, which
    /// is the worst moment to discover that the face on file is not
    /// yours. Turning it off asks nothing: the phone is already unlocked
    /// and in the owner's hand, and demanding a face to *reduce*
    /// security is theatre.
    private var lockBinding: Binding<Bool> {
        Binding(
            get: { preferences.requiresBiometrics },
            set: { wanted in
                guard wanted else {
                    preferences.requiresBiometrics = false
                    return
                }
                Task {
                    let passed = await BiometricLock.authenticate(
                        reason: String(localized: "Turn on locking"))
                    preferences.requiresBiometrics = passed
                }
            })
    }

    private var zoneBinding: Binding<String?> {
        Binding(get: { preferences.timeZoneIdentifier },
                set: { preferences.timeZoneIdentifier = $0 })
    }
}
