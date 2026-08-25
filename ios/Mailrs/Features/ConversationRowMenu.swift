import SwiftUI

/// What a long press on one row offers.
///
/// Split out of `ConversationListView` at the 500-line limit this
/// repository holds every language to — the list's `body` had grown to
/// hold the whole screen, and this is one subject inside it: everything
/// a reader can do to the conversation under their thumb.
struct ConversationRowMenu: View {
    let conversation: Wire.Conversation
    @Environment(Session.self) private var session

    var body: some View {
                    // Pinning is a long-press: it is done once
                    // and read for weeks, unlike the swipes,
                    // which are the fifty-a-day gestures. The
                    // list already came back with this field —
                    // the phone simply had no way to set it and
                    // did not draw the ones the desk had.
                    // Starring. The phone has it on a swipe; the
                    // iPad's leading swipe is not reachable in a
                    // three-column layout and the Mac has no swipes
                    // at all, so the verb existed in `Session` and
                    // could be reached from neither of them. The menu
                    // is the one place all three share.
                    Button {
                        Task { await session.toggleStarred(conversation) }
                    } label: {
                        Label(
                            StarToggle.label(starred: conversation.flagged),
                            systemImage: StarToggle.icon(starred: conversation.flagged)
                        )
                    }
                    .accessibilityIdentifier("row.menu.star")
                    Button {
                        Task { await session.togglePinned(conversation) }
                    } label: {
                        Label(
                            PinLabel.title(pinned: conversation.pinned),
                            systemImage: PinLabel.icon(pinned: conversation.pinned)
                        )
                    }
                    // Away until later. A submenu rather than
                    // four more rows: snoozing is one idea with
                    // four answers, and the menu already holds
                    // the pin, the buckets and the junk verdict.
                    if SnoozeState.isAsleep(conversation, now: Date()) {
                        Button {
                            Task { await session.snooze(conversation, until: nil) }
                        } label: {
                            Label("Wake now", systemImage: "bell")
                        }
                    } else {
                        Menu {
                            ForEach(SnoozeChoice.allCases) { choice in
                                Button(choice.label) {
                                    Task {
                                        await session.snooze(
                                            conversation,
                                            until: choice.fireDate(
                                                after: Date(), calendar: .current))
                                    }
                                }
                            }
                        } label: {
                            Label("Snooze", systemImage: "clock")
                        }
                    }
                    Divider()
                    // Junk lives in the long-press menu, not the
                    // swipe rows — those are full, and a verdict
                    // that trains the filter deserves a deliberate
                    // gesture rather than the one you make fifty
                    // times a day.
                    if session.activeList == .junk {
                        Button {
                            Task { await session.setJunk(conversation, junk: false) }
                        } label: {
                            Label("Not junk", systemImage: "checkmark.shield")
                        }
                    } else {
                        // Where it belongs. The classifier puts
                        // mail in Inbox, Notifications or
                        // Promotions and gets it wrong often
                        // enough that there has to be a way to
                        // say so — the server always had the
                        // verbs; nothing on the phone reached
                        // them.
                        ForEach(MailBucket.offered(from: session.activeList)) { bucket in
                            Button {
                                Task { await session.move(conversation, to: bucket) }
                            } label: {
                                Label(bucket.label, systemImage: bucket.systemImage)
                            }
                        }
                        Divider()
                        Button(role: .destructive) {
                            Task { await session.setJunk(conversation, junk: true) }
                        } label: {
                            Label("Mark as junk", systemImage: "xmark.bin")
                        }
                    }
    }
}
