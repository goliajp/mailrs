import Foundation

/// The reader's own settings. Each call says in the banner when it fails
/// and answers nil (or false), so a screen never shows a value the server
/// did not give or a change it did not take.
extension Session {
    /// Runs `call`; on failure puts `failure` in the banner and answers nil.
    private func settingsCall<T>(
        _ failure: String.LocalizationValue, _ call: (MailrsClient) async throws -> T
    ) async -> T? {
        guard let client else { return nil }
        do {
            return try await call(client)
        } catch {
            banner = String(localized: failure)
            return nil
        }
    }

    func notifyPrefs() async -> SettingsWire.NotifyPrefs? {
        await settingsCall("The notification settings could not be loaded") { try await $0.notifyPrefs() }
    }

    func saveNotifyPrefs(_ p: SettingsWire.NotifyPrefs) async -> Bool {
        await settingsCall("The notification settings were not saved") { try await $0.saveNotifyPrefs(p) } != nil
    }

    func notifyOverrides() async -> SettingsWire.Overrides? {
        await settingsCall("The notification settings could not be loaded") { try await $0.notifyOverrides() }
    }

    func notifySender(_ address: String, _ value: String?) async -> Bool {
        await settingsCall("The notification settings were not saved") { try await $0.notifySender(address, value) } != nil
    }

    func notifyThread(_ threadId: String, _ value: String?) async -> Bool {
        await settingsCall("The notification settings were not saved") { try await $0.notifyThread(threadId, value) } != nil
    }

    func signedInSessions() async -> [SettingsWire.SignedInSession]? {
        await settingsCall("The signed-in devices could not be loaded") { try await $0.signedInSessions() }
    }

    func endSession(_ id: String) async -> Bool {
        await settingsCall("The device was not signed out") { try await $0.endSession(id) } != nil
    }

    func endOtherSessions() async -> Int? {
        await settingsCall("The other devices were not signed out") { try await $0.endOtherSessions() }
    }

    func appPasswords() async -> [SettingsWire.AppPassword]? {
        await settingsCall("The app passwords could not be loaded") { try await $0.appPasswords() }
    }

    func revokeAppPassword(_ id: String) async -> Bool {
        await settingsCall("The app password was not revoked") { try await $0.revokeAppPassword(id) } != nil
    }

    func vacation() async -> SettingsWire.Vacation? {
        await settingsCall("The vacation reply could not be loaded") { try await $0.vacation() }
    }

    func saveVacation(_ v: SettingsWire.Vacation) async -> SettingsWire.Vacation? {
        await settingsCall("The vacation reply was not saved") { try await $0.saveVacation(v) }
    }

    func masks() async -> [SettingsWire.Mask]? {
        await settingsCall("The masked addresses could not be loaded") { try await $0.masks() }
    }

    func createMask(forDomain: String, description: String) async -> SettingsWire.Mask? {
        await settingsCall("The masked address was not created") {
            try await $0.createMask(forDomain: forDomain, description: description)
        }
    }

    func fileSender(_ sender: String, to bucket: SettingsWire.Bucket) async -> Int? {
        await settingsCall("The sender's mail was not filed") { try await $0.fileSender(sender, to: bucket) }
    }

    /// The swipe setting from the server. A failure keeps what is there
    /// and says nothing: the swipes still work, as they did before.
    func loadSwipe() async {
        guard let client else { return }
        if let found = try? await client.swipeSetting() { swipe = found }
    }

    /// Save the swipe setting; the swipes change at once and go back if
    /// the server does not take it.
    func saveSwipe(_ s: SwipeSetting) async {
        let before = swipe
        swipe = s
        if await settingsCall("The swipe setting was not saved", { try await $0.saveSwipeSetting(s) }) == nil {
            swipe = before
        }
    }
}
