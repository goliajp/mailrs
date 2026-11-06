import SwiftUI

/// Whether the Inbox shows its section tabs. Saved on the server, so the
/// web and the Android app follow the same choice.
struct SectionedInboxSettingsSection: View {
    @Environment(Session.self) private var session

    var body: some View {
        Section {
            Toggle("Sectioned inbox", isOn: Binding(
                get: { session.sectionedInbox },
                set: { on in Task { await session.saveSectionedInbox(on) } }
            ))
            .accessibilityIdentifier("settings.sectionedInbox")
        } footer: {
            Text("Tabs over the Inbox for mail to reply to, conversations waiting on an answer and new senders. The same setting is used on the web and in the Android app")
        }
        .task { await session.loadSectionedInbox() }
    }
}
