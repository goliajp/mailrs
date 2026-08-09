import SwiftUI

/// One conversation, as a row.
///
/// Split out of `ConversationListView.swift` at the 500-line limit this
/// repository holds every language to.

struct ConversationRow: View {
    let conversation: Wire.Conversation
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

    private var rowOpacity: Double {
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
                            .overlay(Circle().stroke(Color(.systemBackground), lineWidth: 2))
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
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(.vertical, 2)
        .opacity(rowOpacity)
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
