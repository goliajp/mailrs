import Foundation

/// The sectioned Inbox: a section narrows the Inbox the way the web's
/// section bar does, and New offers to accept its senders.
extension Session {

    /// The cache slot of what is on screen: each section is its own list,
    /// or its rows would overwrite the whole Inbox's.
    var listCacheKey: String {
        guard activeList == .inbox, let inboxSection else { return activeList.rawValue }
        return "\(activeList.rawValue).\(inboxSection.rawValue)"
    }

    /// Show one section of the Inbox, or the whole Inbox with nil.
    func selectSection(_ section: InboxSection?) async {
        guard section != inboxSection else { return }
        inboxSection = section
        let cached = cache.readConversations(list: listCacheKey) ?? []
        conversations = cached
        initialLoading = cached.isEmpty
        reachedEnd = false
        await loadConversations()
    }

    /// Accept the senders of the conversations shown in New, then reload:
    /// accepted senders leave the section, so the list shows what is left.
    func acceptShownSenders() async {
        guard let client else { return }
        let senders = InboxSection.latestSenders(conversations, me: role?.address ?? "")
        guard !senders.isEmpty else { return }
        do {
            _ = try await client.acceptSenders(senders)
            conversations = []
            reachedEnd = false
            await loadConversations()
        } catch {
            banner = String(localized: "The senders could not be accepted")
        }
    }
}
