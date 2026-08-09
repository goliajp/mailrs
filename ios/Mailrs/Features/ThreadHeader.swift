import SwiftUI

/// The thread's title block: subject, count, participants.
///
/// Split out of `ThreadView.swift` at the 500-line limit. `private` came
/// off on the way: in Swift that is file scope, and these are drawn from
/// a file that is no longer this one.

/// The thread's own title block.
///
/// Apple Mail gives the subject a line of its own above the messages;
/// a nav bar cannot, and a subject nobody can read is a mail app's
/// worst small failure.
struct ThreadHeader: View {
    let conversation: Wire.Conversation
    let messages: [Wire.Message]
    let myAddress: String

    /// Everyone who wrote in this thread, in the order they first
    /// appear, minus me — the same rule the row's face follows.
    private var participants: String {
        var seen = Set<String>()
        var names: [String] = []
        for message in messages {
            let email = SenderName.extractEmail(message.sender)
            if email == myAddress || seen.contains(email) { continue }
            seen.insert(email)
            names.append(SenderName.extractName(message.sender))
        }
        return names.joined(separator: ", ")
    }

    @Environment(\.dynamicTypeSize) private var typeSize

    /// Two keys rather than one with a plural rule: the catalog holds
    /// the singular and the count form separately, which is what lets
    /// a language that counts differently say so.
    private var countLabel: LocalizedStringKey {
        if messages.count == 1 { return "1 message" }
        return "\(messages.count) messages"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                ValueOrPlaceholder(value: conversation.subject, placeholder: "(no subject)")
                    .font(.title3.weight(.semibold))
                    // Named so a test can ask this element what it says
                    // rather than asking the screen whether a string is
                    // anywhere on it — the list behind a push can still
                    // answer yes.
                    .accessibilityIdentifier("thread-subject")
                    .lineLimit(RowLayout.threadSubjectLines(typeSize))
                    .fixedSize(horizontal: false, vertical: true)
                if conversation.flagged {
                    Image(systemName: "star.fill")
                        .font(.caption)
                        .foregroundStyle(.yellow)
                        .accessibilityLabel("Starred")
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                Text(countLabel)
                    .layoutPriority(1)
                if !participants.isEmpty {
                    // Wraps at the accessibility sizes rather than
                    // ending at "· Alic…", which names nobody. The
                    // frame is what keeps the second line left: without
                    // it the wrapped text centres in what the HStack
                    // gave it, and the tail sat against the right edge
                    // with a gap in front of it.
                    Text(verbatim: "· \(participants)")
                        .lineLimit(RowLayout.senderLines(typeSize))
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12)
        .padding(.top, 4)
        .padding(.bottom, 2)
    }
}
