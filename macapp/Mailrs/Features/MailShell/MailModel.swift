import Foundation
import Observation

@MainActor
@Observable
final class MailModel {
    // MARK: data
    private(set) var conversations: [ConversationSummary] = []
    var selectedThreadId: String?

    // MARK: filters (drive list refresh)
    var folder: MailFolder = .inbox { didSet { if folder != oldValue { Task { await refresh() } } } }
    var category: MailCategory? { didSet { if category != oldValue { Task { await refresh() } } } }
    var quickFilter: QuickFilter = .all { didSet { if quickFilter != oldValue { Task { await refresh() } } } }
    var sortOrder: SortOrder = .newest
    var searchQuery: String = ""

    // MARK: loading state
    private(set) var isInitialLoading = false
    private(set) var isLoadingMore = false
    private(set) var hasMore = true
    var errorMessage: String?
    private(set) var connectionStatus: ConnectionStatus = .offline

    private let service: ConversationService
    private var currentLoadTask: Task<Void, Never>?
    private var eventsConsumerTask: Task<Void, Never>?
    private var statusConsumerTask: Task<Void, Never>?
    private var pollTask: Task<Void, Never>?

    private weak var eventsClient: EventsClient?
    private weak var threadModel: ThreadModel?
    private var currentUserAddress: String = ""
    private var notificationsEnabled: Bool = true
    private var notificationSoundEnabled: Bool = true

    init(service: ConversationService) {
        self.service = service
    }

    // MARK: derived

    /// Client-side-filtered view (for `attachment` quick filter + sort order).
    /// The server already applied folder/category/unread/starred.
    var visibleConversations: [ConversationSummary] {
        var list = conversations
        // `attachment` has no server-side filter — we'd need message-level data
        // to know, which isn't in ConversationSummary. Skip in M2; M3 can wire it.
        switch sortOrder {
        case .newest: list.sort { $0.last_date > $1.last_date }
        case .oldest: list.sort { $0.last_date < $1.last_date }
        case .unread: list.sort {
            if $0.isUnread != $1.isUnread { return $0.isUnread && !$1.isUnread }
            return $0.last_date > $1.last_date
        }
        }
        return list
    }

    var totalUnread: Int {
        conversations.reduce(0) { $0 + $1.unread_count }
    }

    // MARK: actions

    func refresh() async {
        currentLoadTask?.cancel()
        let task = Task { [weak self] in
            guard let self else { return }
            await self.loadInitial()
        }
        currentLoadTask = task
        await task.value
    }

    private func loadInitial() async {
        isInitialLoading = true
        errorMessage = nil
        defer { isInitialLoading = false }

        do {
            let opts = currentOptions(before: nil)
            let data: [ConversationSummary]
            if searchQuery.trimmingCharacters(in: .whitespaces).isEmpty {
                data = try await service.list(opts)
            } else {
                data = try await service.search(searchQuery, options: opts)
            }
            if Task.isCancelled { return }
            conversations = data
            hasMore = data.count >= opts.limit
        } catch {
            if Task.isCancelled { return }
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func loadMoreIfNeeded(for conversation: ConversationSummary) async {
        // Trigger when the visible threshold reaches the last row.
        guard hasMore, !isLoadingMore, !isInitialLoading else { return }
        guard conversation.thread_id == conversations.last?.thread_id else { return }
        guard searchQuery.trimmingCharacters(in: .whitespaces).isEmpty else { return } // search has no pagination
        guard let last = conversations.last else { return }

        isLoadingMore = true
        defer { isLoadingMore = false }

        do {
            let opts = currentOptions(before: last.lastDateUnixSeconds)
            let next = try await service.list(opts)
            if Task.isCancelled { return }
            let existing = Set(conversations.map(\.thread_id))
            let fresh = next.filter { !existing.contains($0.thread_id) }
            conversations.append(contentsOf: fresh)
            hasMore = next.count >= opts.limit
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
        }
    }

    func runSearch() async {
        await refresh()
    }

    func clearSearch() async {
        if !searchQuery.isEmpty {
            searchQuery = ""
            await refresh()
        }
    }

    // MARK: optimistic updates (called from ThreadView after successful actions)

    func markConversationUnread(threadId: String) {
        update(threadId: threadId) { $0.unread_count = max(1, $0.unread_count) }
    }

    func markConversationRead(threadId: String) {
        update(threadId: threadId) { $0.unread_count = 0 }
    }

    func setFlagged(threadId: String, flagged: Bool) {
        update(threadId: threadId) { $0.flagged = flagged }
    }

    private func update(threadId: String, _ mutate: (inout ConversationSummary) -> Void) {
        guard let idx = conversations.firstIndex(where: { $0.thread_id == threadId }) else { return }
        var copy = conversations[idx]
        mutate(&copy)
        conversations[idx] = copy
    }

    // MARK: realtime wiring

    func attachRealtime(client: EventsClient, threadModel: ThreadModel, userAddress: String) {
        self.eventsClient = client
        self.threadModel = threadModel
        self.currentUserAddress = userAddress

        eventsConsumerTask?.cancel()
        eventsConsumerTask = Task { [weak self, events = client.events] in
            for await event in events {
                await self?.handle(event: event)
            }
        }

        statusConsumerTask?.cancel()
        statusConsumerTask = Task { [weak self, statuses = client.statuses] in
            for await status in statuses {
                await self?.setConnectionStatus(status)
            }
        }

        startPolling()
    }

    func detachRealtime() {
        eventsConsumerTask?.cancel(); eventsConsumerTask = nil
        statusConsumerTask?.cancel(); statusConsumerTask = nil
        pollTask?.cancel(); pollTask = nil
        eventsClient = nil
        threadModel = nil
        currentUserAddress = ""
        connectionStatus = .offline
    }

    func applyNotificationPreferences(enabled: Bool, sound: Bool) {
        notificationsEnabled = enabled
        notificationSoundEnabled = sound
    }

    private func setConnectionStatus(_ status: ConnectionStatus) {
        connectionStatus = status
    }

    private func handle(event: MailEvent) async {
        switch event {
        case .other: break
        case .newMessage(let e):
            guard e.user == currentUserAddress else { return }
            await refreshIncremental()
            if let tid = selectedThreadId, tid == e.thread_id {
                if let threadModel {
                    await threadModel.load(threadId: tid)
                }
            }
            await maybePostNotification(for: e)
            NotificationManager.shared.setBadgeCount(totalUnread)
        }
    }

    private func maybePostNotification(for event: NewMessageEvent) async {
        guard notificationsEnabled else { return }
        if NotificationManager.isForegroundActive { return }
        let ok = await NotificationManager.shared.requestAuthorizationIfNeeded()
        guard ok else { return }
        NotificationManager.shared.postNewMessage(event, withSound: notificationSoundEnabled)
    }

    /// Merge-refresh: fetch top N and update in-place. Unlike `refresh()`
    /// this does not clear existing items not in the fresh window.
    private func refreshIncremental() async {
        do {
            let opts = currentOptions(before: nil)
            let fresh: [ConversationSummary]
            if searchQuery.trimmingCharacters(in: .whitespaces).isEmpty {
                fresh = try await service.list(opts)
            } else {
                fresh = try await service.search(searchQuery, options: opts)
            }
            var merged: [ConversationSummary] = []
            var seen = Set<String>()
            for c in fresh {
                merged.append(c)
                seen.insert(c.thread_id)
            }
            for c in conversations where !seen.contains(c.thread_id) {
                merged.append(c)
            }
            conversations = merged
        } catch {
            // silent — polling / next event will retry
        }
    }

    private func startPolling() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 60 * 1_000_000_000)
                guard let self else { return }
                if await self.connectionStatus != .connected {
                    await self.refreshIncremental()
                }
            }
        }
    }

    // MARK: private

    private func currentOptions(before: Int64?) -> ConversationListOptions {
        ConversationListOptions(
            limit: 50,
            before: before,
            category: category,
            folder: folder,
            quickFilter: quickFilter,
            domains: [],
            archived: false,
            section: nil
        )
    }
}
