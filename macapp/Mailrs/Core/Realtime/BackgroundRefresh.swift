import Foundation

#if os(iOS)
import BackgroundTasks
import UIKit

/// Registers and schedules an iOS BGAppRefreshTask that polls the server for
/// new conversations while the app is in the background. Best-effort — iOS
/// decides when (if ever) to fire.
@MainActor
final class BackgroundRefreshManager {
    static let identifier = "jp.golia.mailrs.refresh"
    private static let lastSeenThreadsKey = "mailrs_bg_last_seen_threads"

    private weak var appModel: AppModel?

    init(appModel: AppModel) {
        self.appModel = appModel
    }

    func register() {
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: Self.identifier,
            using: nil
        ) { [weak self] task in
            guard let task = task as? BGAppRefreshTask else { return }
            Task { @MainActor [weak self] in
                await self?.handle(task: task)
            }
        }
    }

    func scheduleNext() {
        let request = BGAppRefreshTaskRequest(identifier: Self.identifier)
        request.earliestBeginDate = Date().addingTimeInterval(15 * 60)
        do {
            try BGTaskScheduler.shared.submit(request)
        } catch {
            // Scheduler rejected — typically during simulator / foreground; ignore.
        }
    }

    private func handle(task: BGAppRefreshTask) async {
        scheduleNext()  // reschedule immediately so we get future fires

        guard let app = appModel,
              app.authStore.isAuthenticated,
              let userAddress = app.authStore.authInfo?.address else {
            task.setTaskCompleted(success: false)
            return
        }

        let deadline = Task<Void, Never> {
            try? await Task.sleep(nanoseconds: 25 * 1_000_000_000)
        }
        task.expirationHandler = {
            deadline.cancel()
        }

        do {
            let fresh = try await app.conversationService.list(
                ConversationListOptions(limit: 20)
            )
            let previouslySeen = Self.loadSeenThreadIds()
            let newOnes = fresh.filter { !previouslySeen.contains($0.thread_id) }

            let enabled = UserDefaults.standard.object(forKey: SettingsKey.notificationsEnabled) as? Bool ?? true
            let sound = UserDefaults.standard.object(forKey: SettingsKey.notificationSound) as? Bool ?? true

            if enabled {
                for convo in newOnes where convo.unread_count > 0 {
                    let event = NewMessageEvent(
                        sender: convo.last_sender,
                        snippet: convo.snippet,
                        subject: convo.subject,
                        thread_id: convo.thread_id,
                        user: userAddress
                    )
                    NotificationManager.shared.postNewMessage(event, withSound: sound)
                }
            }

            let totalUnread = fresh.reduce(0) { $0 + $1.unread_count }
            NotificationManager.shared.setBadgeCount(totalUnread)
            Self.saveSeenThreadIds(Set(fresh.map(\.thread_id)))
            task.setTaskCompleted(success: true)
        } catch {
            task.setTaskCompleted(success: false)
        }
        _ = deadline
    }

    private static func loadSeenThreadIds() -> Set<String> {
        guard let arr = UserDefaults.standard.array(forKey: lastSeenThreadsKey) as? [String] else {
            return []
        }
        return Set(arr)
    }

    private static func saveSeenThreadIds(_ ids: Set<String>) {
        // Bound to reasonable size.
        let capped = Array(ids.prefix(200))
        UserDefaults.standard.set(capped, forKey: lastSeenThreadsKey)
    }
}

#else

/// Stub on non-iOS platforms so call sites stay clean.
@MainActor
final class BackgroundRefreshManager {
    init(appModel: AppModel) {}
    func register() {}
    func scheduleNext() {}
}

#endif
