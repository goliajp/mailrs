import SwiftUI

/// One mailbox in a sidebar, on iPad or on the Mac.
///
/// Shared because the two sidebars answer the same question and a
/// second copy would drift — the iPad's rows gaining a count the Mac's
/// did not, or the two disagreeing about what the count means.
///
/// **The badge is only on the list being shown.** The server gives one
/// unseen total, not a count per mailbox, so a number beside every row
/// would be the same number seven times — which reads as "there are
/// four unread in Archived" and is false. Showing it only where it is
/// true is less information and no wrong information.
struct SidebarMailboxRow: View {
    let list: MailList
    /// How many unread the *current* list holds, or nil for the rest.
    let unread: Int?

    var body: some View {
        Label(list.title, systemImage: list.systemImage)
            .badge(unread ?? 0)
            .accessibilityIdentifier("mailbox.\(list.rawValue)")
    }
}

extension MailList {
    /// The unread count to show beside this row.
    ///
    /// `nil` for every list but the one on screen — see `SidebarMailboxRow`.
    /// A badge of 0 draws nothing, so this reads as "show it here and
    /// nowhere else" rather than as a special case at the call site.
    func badgeCount(activeList: MailList, unreadInActive: Int) -> Int? {
        self == activeList && unreadInActive > 0 ? unreadInActive : nil
    }
}
