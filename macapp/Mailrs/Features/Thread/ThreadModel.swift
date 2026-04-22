import Foundation
import Observation

@MainActor
@Observable
final class ThreadModel {
    private(set) var messages: [ThreadMessage] = []
    private(set) var isLoading = false
    private(set) var errorMessage: String?

    private(set) var currentThreadId: String?

    private let threadService: ThreadService
    private let actionService: MessageActionService
    private var loadTask: Task<Void, Never>?

    init(threadService: ThreadService, actionService: MessageActionService) {
        self.threadService = threadService
        self.actionService = actionService
    }

    func load(threadId: String) async {
        loadTask?.cancel()
        currentThreadId = threadId
        isLoading = true
        errorMessage = nil
        messages = []

        let task = Task { [weak self] in
            guard let self else { return }
            do {
                let msgs = try await self.threadService.fetch(threadId: threadId)
                if Task.isCancelled { return }
                self.messages = msgs
                // Auto-mark read after loading a thread with unread messages.
                if msgs.contains(where: { !$0.isSeen }) {
                    try? await self.actionService.markRead(threadId: threadId)
                }
            } catch {
                if !Task.isCancelled {
                    self.errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
                }
            }
            self.isLoading = false
        }
        loadTask = task
        await task.value
    }

    func clear() {
        loadTask?.cancel()
        currentThreadId = nil
        messages = []
        isLoading = false
        errorMessage = nil
    }

    // MARK: actions

    func markUnread() async -> Bool {
        guard let id = currentThreadId else { return false }
        do {
            try await actionService.markUnread(threadId: id)
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func toggleStar(currentlyStarred: Bool) async -> Bool {
        guard let id = currentThreadId else { return false }
        do {
            if currentlyStarred {
                try await actionService.unstar(threadId: id)
            } else {
                try await actionService.star(threadId: id)
            }
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func archive() async -> Bool {
        guard let id = currentThreadId else { return false }
        do {
            try await actionService.archive(threadId: id)
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func delete() async -> Bool {
        guard let id = currentThreadId else { return false }
        do {
            try await threadService.delete(threadId: id)
            clear()
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func snooze(until: Date) async -> Bool {
        guard let id = currentThreadId else { return false }
        do {
            try await actionService.snooze(threadId: id, until: until)
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }

    func feedback(senderEmail: String, action: FeedbackAction) async -> Bool {
        do {
            try await actionService.feedback(senderEmail: senderEmail, action: action)
            return true
        } catch {
            errorMessage = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            return false
        }
    }
}
