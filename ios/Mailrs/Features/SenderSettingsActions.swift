import SwiftUI

/// In a sender's menu: where their mail is always filed, and whether it
/// notifies. Each is a setting the web keeps under Settings, reached here
/// from the message that prompted it.
struct SenderSettingsActions: View {
    @Environment(Session.self) private var session
    let address: String

    var body: some View {
        Menu {
            ForEach(SettingsWire.Bucket.allCases) { bucket in
                Button(SenderSettingsActions.bucketLabel(bucket)) {
                    Task { await file(to: bucket) }
                }
                .accessibilityIdentifier("sender.file.\(bucket.rawValue)")
            }
        } label: {
            Label("Always file to…", systemImage: "tray.and.arrow.down")
        }
        .accessibilityIdentifier("sender.file")
        Menu {
            Button("Always notify") { Task { await notify("always") } }
                .accessibilityIdentifier("sender.notify.always")
            Button("Never notify") { Task { await notify("never") } }
                .accessibilityIdentifier("sender.notify.never")
            Button("As my notification setting says") { Task { await notify(nil) } }
                .accessibilityIdentifier("sender.notify.default")
        } label: {
            Label("Notifications from this sender", systemImage: "bell")
        }
        .accessibilityIdentifier("sender.notify")
    }

    private func file(to bucket: SettingsWire.Bucket) async {
        guard let moved = await session.fileSender(address, to: bucket) else { return }
        session.banner = String(localized: "Filed. \(moved) conversations moved")
    }

    private func notify(_ value: String?) async {
        if await session.notifySender(address, value) {
            session.banner = String(localized: "Notification setting saved")
        }
    }

    static func bucketLabel(_ bucket: SettingsWire.Bucket) -> LocalizedStringKey {
        switch bucket {
        case .inbox: "Inbox"
        case .notifications: "Notifications"
        case .promotions: "Promotions"
        }
    }
}

/// In a conversation's More menu: follow it, mute it, or leave it to the
/// notification setting.
struct ThreadNotifyActions: View {
    @Environment(Session.self) private var session
    let threadId: String

    var body: some View {
        Menu {
            Button("Notify for every reply") { Task { await set("follow") } }
                .accessibilityIdentifier("thread.notify.follow")
            Button("Mute") { Task { await set("mute") } }
                .accessibilityIdentifier("thread.notify.mute")
            Button("As my notification setting says") { Task { await set(nil) } }
                .accessibilityIdentifier("thread.notify.default")
        } label: {
            Label("Notifications for this conversation", systemImage: "bell")
        }
        .accessibilityIdentifier("thread.notify")
    }

    private func set(_ value: String?) async {
        if await session.notifyThread(threadId, value) {
            session.banner = String(localized: "Notification setting saved")
        }
    }
}
