import SwiftUI

/// Push: which mail notifies, the quiet hours, whether a lock screen shows
/// who wrote, and the senders and conversations set apart from the rule.
struct NotifySettingsView: View {
    @Environment(Session.self) private var session
    /// The zone chosen in the app, which the time pickers show: the quiet
    /// hours are read and stamped in it, not in the phone's.
    @Environment(\.timeZone) private var timeZone
    @State private var prefs: SettingsWire.NotifyPrefs?
    @State private var overrides: SettingsWire.Overrides?

    private let removal = PushRemoval.shared

    var body: some View {
        Form {
            if removal.removedElsewhere {
                Section {
                    Text("Removed from another device; turn on again to receive pushes")
                        .accessibilityIdentifier("notify.removedElsewhere")
                    Button("Turn on again") {
                        removal.turnOnAgain()
                        PushRegistrar.requestAuthorization()
                    }
                    .accessibilityIdentifier("notify.turnOnAgain")
                }
            }
            if let prefs {
                Section {
                    Picker("Notify me about", selection: field(\.mode)) {
                        Text("Mail to answer and important people").tag("default")
                        Text("Every message").tag("all")
                        Text("Important people only").tag("important")
                        Text("Nothing").tag("off")
                    }
                    .accessibilityIdentifier("notify.mode")
                    Toggle("Hide sender and subject", isOn: field(\.hideContent))
                        .accessibilityIdentifier("notify.hideContent")
                }
                Section {
                    Toggle("Quiet hours", isOn: quietOn)
                        .accessibilityIdentifier("notify.quiet")
                    if prefs.quietStart != nil {
                        DatePicker("Starts", selection: quietTime(\.quietStart), displayedComponents: .hourAndMinute)
                            .accessibilityIdentifier("notify.quietStart")
                        DatePicker("Ends", selection: quietTime(\.quietEnd), displayedComponents: .hourAndMinute)
                            .accessibilityIdentifier("notify.quietEnd")
                    }
                } footer: {
                    Text("Read in this phone's time zone")
                }
            }
            if let overrides {
                OverrideSection(title: "Senders", rows: overrides.senders, id: "notify.sender") { key in
                    if await session.notifySender(key, nil) { await load() }
                }
                OverrideSection(title: "Conversations", rows: overrides.threads, id: "notify.thread") { key in
                    if await session.notifyThread(key, nil) { await load() }
                }
            }
        }
        .navigationTitle("Notifications")
        .task { await load() }
    }

    private func load() async {
        prefs = await session.notifyPrefs()
        overrides = await session.notifyOverrides()
    }

    /// A field of the preferences, saved as soon as it changes.
    private func field<T>(_ path: WritableKeyPath<SettingsWire.NotifyPrefs, T>) -> Binding<T> {
        Binding(
            get: { prefs![keyPath: path] },
            set: { value in
                guard var next = prefs else { return }
                next[keyPath: path] = value
                save(next)
            })
    }

    /// Quiet hours on: 22:00 to 07:00 until the reader picks other times.
    private var quietOn: Binding<Bool> {
        Binding(
            get: { prefs?.quietStart != nil },
            set: { on in
                guard var next = prefs else { return }
                next.quietStart = QuietHours.start(on: on)
                next.quietEnd = QuietHours.end(on: on)
                save(next)
            })
    }

    private func quietTime(_ path: WritableKeyPath<SettingsWire.NotifyPrefs, Int?>) -> Binding<Date> {
        Binding(
            get: { QuietHours.date(minutes: prefs?[keyPath: path] ?? 0, in: timeZone) },
            set: { date in
                guard var next = prefs else { return }
                next[keyPath: path] = QuietHours.minutesAfterMidnight(of: date, in: timeZone)
                save(next)
            })
    }

    /// Shown at once, saved behind; a refusal puts the old value back.
    private func save(_ next: SettingsWire.NotifyPrefs) {
        var stamped = next
        stamped.utcOffsetMinutes = timeZone.secondsFromGMT() / 60
        let before = prefs
        prefs = stamped
        Task {
            if !(await session.saveNotifyPrefs(stamped)) { prefs = before }
        }
    }
}

private struct OverrideSection: View {
    let title: LocalizedStringKey
    let rows: [[String]]
    let id: String
    let remove: (String) async -> Void

    var body: some View {
        if !rows.isEmpty {
            Section(title) {
                ForEach(rows, id: \.self) { row in
                    HStack {
                        Text(row.first ?? "")
                        Spacer()
                        Text(OverrideSection.label(row.last ?? "")).foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("\(id).\(row.first ?? "")")
                    .swipeActions {
                        Button("Remove", role: .destructive) { Task { await remove(row.first ?? "") } }
                    }
                }
            }
        }
    }

    static func label(_ value: String) -> LocalizedStringKey {
        switch value {
        case "always": "Always notify"
        case "never": "Never notify"
        case "follow": "Every reply"
        case "mute": "Muted"
        default: LocalizedStringKey(value)
        }
    }
}

/// Quiet hours as minutes after midnight, the server's unit.
enum QuietHours {
    static func start(on: Bool) -> Int? {
        if on { return 22 * 60 }
        return nil
    }

    static func end(on: Bool) -> Int? {
        if on { return 7 * 60 }
        return nil
    }

    static func date(minutes: Int, in zone: TimeZone) -> Date {
        calendar(zone).date(bySettingHour: minutes / 60, minute: minutes % 60, second: 0, of: Date()) ?? Date()
    }

    static func minutesAfterMidnight(of date: Date, in zone: TimeZone) -> Int {
        let c = calendar(zone).dateComponents([.hour, .minute], from: date)
        return (c.hour ?? 0) * 60 + (c.minute ?? 0)
    }

    private static func calendar(_ zone: TimeZone) -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone
        return calendar
    }
}
