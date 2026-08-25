import SwiftUI

/// One conversation, as a row.
///
/// Split out of `ConversationListView.swift` at the 500-line limit this
/// repository holds every language to.

struct ConversationRow: View {
    let conversation: Wire.Conversation
    /// True while the list is in select mode.
    var isSelecting = false
    @Environment(Session.self) private var session
    @Environment(\.dynamicTypeSize) private var typeSize

    /// Read rows recede, the web's `muted`. Unread already carries the
    /// dot and the weight; dimming what is done is what makes a long
    /// list scannable rather than uniformly loud.
    private var senderWeight: Font.Weight {
        if conversation.unreadCount > 0 { return .semibold }
        return .regular
    }

    private var importanceColor: Color {
        if conversation.importanceLevel == "critical" { return .red }
        return .orange
    }

    private var importanceLabel: LocalizedStringKey {
        if conversation.importanceLevel == "critical" { return "Critical" }
        return "Important"
    }

    /// Read rows recede — except while choosing, when every row is a
    /// target and dimming half of them makes the choice harder to see.
    /// Reported from the phone as not being able to tell what was
    /// selected.
    private var rowOpacity: Double {
        if isSelecting { return 1 }
        if conversation.unreadCount > 0 { return 1 }
        return 0.7
    }

    private var sender: String {
        SenderName.rowFace(
            participants: conversation.participants,
            myAddress: session.myAddress
        )
    }

    /// Whose color and initial the avatar wears — the same participant
    /// the name comes from.
    private var face: String {
        let mine = session.myAddress
        let others = conversation.participants.filter { SenderName.extractEmail($0) != mine }
        return others.first ?? conversation.participants.first ?? ""
    }

    private var extraParticipants: Int {
        let mine = session.myAddress
        let others = conversation.participants.filter { SenderName.extractEmail($0) != mine }
        return max(0, others.count - 1)
    }

    /// The web row's count chip: received and sent split when the
    /// thread has both directions, a plain total otherwise.
    private var countLabel: String? {
        guard conversation.messageCount > 1 else { return nil }
        if conversation.sentCount > 0, conversation.receivedCount > 0 {
            return "\(conversation.receivedCount)↓ \(conversation.sentCount)↑"
        }
        return "×\(conversation.messageCount)"
    }

    var body: some View {
        HStack(alignment: RowLayout.gutterAlignment(typeSize), spacing: 10) {
            // The avatar wears the unread state as a badge on its rim —
            // the dot keeps its VoiceOver label ("Unread"), because a
            // colour is not a label.
            SenderAvatar(sender: face)
                .overlay(alignment: .topTrailing) {
                    if conversation.unreadCount > 0 {
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 11, height: 11)
                            .overlay(Circle().stroke(Color.pageBackground, lineWidth: 2))
                            .offset(x: 1, y: -1)
                            .accessibilityHidden(false)
                            .accessibilityLabel("Unread")
                    }
                }

            VStack(alignment: .leading, spacing: 1) {
                header
                HStack(spacing: 4) {
                    ValueOrPlaceholder(value: conversation.subject, placeholder: "(no subject)")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(RowLayout.subjectLines(typeSize))
                    if conversation.flagged {
                        Image(systemName: "star.fill")
                            .font(.caption2)
                            .foregroundStyle(.yellow)
                            .accessibilityLabel("Starred")
                    }
                    // Why this row is at the top. Without it a pinned
                    // thread reads as the newest one, and the next
                    // arrival that does not displace it looks like a
                    // list that has stopped sorting.
                    if conversation.pinned {
                        Image(systemName: "pin.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Pinned")
                    }
                    Spacer(minLength: 0)
                }
                // The line that answers "do I need to open this".
                //
                // The server has sent it all along — 31,182 of 31,335
                // rows on prod carry one — and this client drew the
                // subject and stopped. A subject says what a message is
                // filed under; the first line says what it wants.
                //
                // Hidden at accessibility sizes, where the subject
                // already takes three lines and a fourth would push the
                // next row off the screen entirely.
                if !typeSize.isAccessibilitySize, !conversation.snippet.isEmpty {
                    Text(conversation.snippet)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 2)
        .opacity(rowOpacity)
        // Named, so a test can address a conversation row rather than
        // whatever list happens to be first on screen. Without this an
        // iPad test swiped the *sidebar* and reported that swiping a
        // conversation offered nothing — true of the row it reached,
        // and not of the row it meant.
        // Named, so a test can address a conversation row rather than
        // whatever list happens to be first on screen. Without this an
        // iPad test swiped the *sidebar* and reported that swiping a
        // conversation offered nothing — true of the row it reached,
        // and not of the row it meant.
        //
        // **Not `.accessibilityElement(children: .combine)`.** That
        // would stop the identifier propagating to the avatar and
        // spare a test one awkward query — and it also takes the
        // swipe actions out of reach, because the row stops being the
        // element the gesture lands on. Making the interface less
        // usable so a test can find things more easily is the wrong
        // trade; the test asks for a hittable element instead.
        .accessibilityIdentifier("row.conversation.\(conversation.threadId)")
    }

    /// Name, who else is on it, how many messages, when.
    ///
    /// One line while they fit; stacked once the reader's text size
    /// means they cannot. Side by side at accessibility sizes the name
    /// lost to the date — see `RowLayout`.
    @ViewBuilder
    private var header: some View {
        if RowLayout.stacksHeader(typeSize) {
            VStack(alignment: .leading, spacing: 2) {
                name
                HStack(spacing: 6) {
                    countChip
                    RowDateText(epochSeconds: conversation.lastDate)
                    Spacer(minLength: 0)
                }
            }
        } else {
            HStack(spacing: 6) {
                name
                Spacer(minLength: 4)
                countChip
                RowDateText(epochSeconds: conversation.lastDate)
            }
        }
    }

    /// The sender, and the two marks that belong beside the name rather
    /// than beside the date: how many other people are on the thread,
    /// and whether it was judged important.
    @ViewBuilder
    private var name: some View {
        HStack(spacing: 6) {
            Text(sender)
                .font(.subheadline.weight(senderWeight))
                .lineLimit(RowLayout.senderLines(typeSize))
            if extraParticipants > 0 {
                Text("+\(extraParticipants)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if conversation.importanceLevel == "critical"
                || conversation.importanceLevel == "important" {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.caption2)
                    .foregroundStyle(importanceColor)
                    .accessibilityLabel(importanceLabel)
            }
        }
    }

    @ViewBuilder
    private var countChip: some View {
        if let countLabel {
            Text(countLabel)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .padding(.horizontal, 4)
                .padding(.vertical, 1)
                .background(Color(.tertiarySystemFill), in: Capsule())
        }
    }
}
