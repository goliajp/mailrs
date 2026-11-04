import SwiftUI

/// Where this account is signed in, and signing the other places out.
struct SessionsView: View {
    @Environment(Session.self) private var session
    @State private var rows: [SettingsWire.SignedInSession]?

    var body: some View {
        List {
            if let rows {
                ForEach(rows) { row in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(SessionsView.deviceLabel(row.userAgent))
                        HStack(spacing: 6) {
                            Text(row.ip)
                            Text(Date(timeIntervalSince1970: TimeInterval(row.lastSeen)), style: .relative)
                            if row.current { Text("This device").foregroundStyle(Color.accentColor) }
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("session.\(row.id)")
                    .swipeActions {
                        if !row.current {
                            Button("Sign out", role: .destructive) { Task { await end(row.id) } }
                        }
                    }
                }
                if rows.contains(where: { !$0.current }) {
                    Button("Sign out everywhere else", role: .destructive) { Task { await endOthers() } }
                        .accessibilityIdentifier("sessions.endOthers")
                }
            }
        }
        .navigationTitle("Signed-in devices")
        .task { rows = await session.signedInSessions() }
    }

    static func deviceLabel(_ userAgent: String) -> String {
        if userAgent.isEmpty { return String(localized: "Unknown device") }
        return userAgent
    }

    private func end(_ id: String) async {
        if await session.endSession(id) { rows = await session.signedInSessions() }
    }

    private func endOthers() async {
        if await session.endOtherSessions() != nil { rows = await session.signedInSessions() }
    }
}

/// App passwords for mail programs: listed and revoked here, made on the
/// web, where the new password can be copied into the program.
struct AppPasswordsView: View {
    @Environment(Session.self) private var session
    @Environment(\.openURL) private var openURL
    @State private var rows: [SettingsWire.AppPassword]?

    var body: some View {
        List {
            if let rows {
                if rows.isEmpty {
                    Text("No app passwords").foregroundStyle(.secondary)
                }
                ForEach(rows) { row in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.name)
                        LastUsedLine(lastUsedAt: row.lastUsedAt)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("appPassword.\(row.id)")
                    .swipeActions {
                        Button("Revoke", role: .destructive) { Task { await revoke(row.id) } }
                    }
                }
            }
            Section {
                Button("Make one on the web") {
                    openURL(SettingsWire.appPasswordsPage(server: session.baseURL))
                }
                .accessibilityIdentifier("appPasswords.web")
            } footer: {
                Text("A new password is shown once, on the web, to copy into the mail program")
            }
        }
        .navigationTitle("App passwords")
        .task { rows = await session.appPasswords() }
    }

    private func revoke(_ id: String) async {
        if await session.revokeAppPassword(id) { rows = await session.appPasswords() }
    }
}

private struct LastUsedLine: View {
    let lastUsedAt: Int64?

    var body: some View {
        Group {
            if let lastUsedAt {
                Text("Last used \(Date(timeIntervalSince1970: TimeInterval(lastUsedAt)), style: .relative) ago")
            } else {
                Text("Never used")
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}
