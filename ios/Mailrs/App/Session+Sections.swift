import Foundation

/// Moving between lists, and the sectioned Inbox: a section narrows the
/// Inbox the way the web's section bar does, and New offers to accept its
/// senders.
extension Session {

    /// The cache slot of what is on screen: each section is its own list,
    /// or its rows would overwrite the whole Inbox's.
    var listCacheKey: String {
        guard activeList == .inbox, let inboxSection else { return activeList.rawValue }
        return "\(activeList.rawValue).\(inboxSection.rawValue)"
    }

    /// Switch lists.
    ///
    /// Everything scoped to the old list goes with it: the rows, the
    /// paging cursor's end flag, and any search. Carrying a search across
    /// would leave Junk showing hits from Inbox, and carrying `reachedEnd`
    /// would leave a fresh list unable to page.
    func select(_ list: MailList) async {
        clearUndo()
        guard list != activeList else { return }
        activeList = list
        inboxSection = nil
        // The old rows cleared and `initialLoading` set here rather than
        // inside the load.
        //
        // `loadConversations` is awaited, and the await yields: for that
        // frame the rows were empty and nothing said a load was running,
        // so SwiftUI drew the empty state. Every switch to an uncached
        // list flashed "All caught up" at a mailbox that was about to
        // fill. Reported from the phone as loading and empty being
        // confused, which is what it is — the view was asked a question
        // it did not yet have the evidence to answer.
        conversations = []
        initialLoading = true
        searchResults = []
        searchQuery = nil
        reachedEnd = false
        sendRows = []
        scheduledSends = []
        if list == .send {
            initialLoading = true
            await loadSendRows()
            // In the same breath: this screen is the only place a
            // scheduled message can be stopped, and one that loads a
            // beat later is one the reader has already scrolled past.
            await loadScheduled()
        } else {
            await loadConversations()
        }
    }

    /// Show one section of the Inbox, or the whole Inbox with nil.
    func selectSection(_ section: InboxSection?) async {
        guard section != inboxSection else { return }
        inboxSection = section
        conversations = []
        initialLoading = true
        reachedEnd = false
        await loadConversations()
    }

    /// The sectioned inbox setting from the server. A failure keeps what is
    /// there and says nothing: the Inbox still works either way.
    func loadSectionedInbox() async {
        guard let client else { return }
        guard let found = try? await client.sectionedInboxSetting() else { return }
        await showSections(found.enabled)
    }

    /// Turn the section tabs on or off; they change at once and go back if
    /// the server does not take it.
    func saveSectionedInbox(_ enabled: Bool) async {
        guard let client else { return }
        let before = sectionedInbox
        await showSections(enabled)
        do {
            _ = try await client.saveSectionedInboxSetting(SectionedInboxSetting(enabled: enabled))
        } catch {
            banner = AppText.string("The sectioned inbox setting was not saved")
            await showSections(before)
        }
    }

    /// With the tabs gone a section can no longer be left, so the whole
    /// Inbox comes back in its place.
    private func showSections(_ enabled: Bool) async {
        sectionedInbox = enabled
        if !enabled { await selectSection(nil) }
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
            banner = AppText.string("The senders could not be accepted")
        }
    }
}
