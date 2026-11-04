import SwiftUI

/// A team conversation's internal notes, under its messages. Read only on
/// the phone: notes are written on the web. Marked as the team's own, the
/// way the web marks them: a note is never part of anything sent.
struct TeamNotesSection: View {
    @Environment(Session.self) private var session
    let conversation: Wire.Conversation
    @State private var notes: [TeamWire.Note]?

    var body: some View {
        if let c = TeamWire.Conv(conversation.sharedConv) {
            VStack(alignment: .leading, spacing: 6) {
                Label("Internal notes · Only the team sees these. They are never sent",
                      systemImage: "lock")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.orange)
                if let notes {
                    if notes.isEmpty {
                        Text("No notes yet").font(.caption).foregroundStyle(.secondary)
                    }
                    ForEach(notes) { note in
                        NoteRow(note: note)
                    }
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.orange.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("team.notes")
            .task(id: conversation.threadId) { notes = await session.teamNotes(c) }
        }
    }
}

private struct NoteRow: View {
    let note: TeamWire.Note

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(note.text).font(.subheadline)
            HStack(spacing: 4) {
                Text(TeamWire.localPart(note.by))
                Text(Date(timeIntervalSince1970: TimeInterval(note.at)), style: .date)
                if note.editedAt != nil { Text("edited") }
            }
            .font(.caption2)
            .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("team.note.\(note.id)")
    }
}

/// Above a reply to a team conversation: who else is on it, so two people
/// do not answer the same customer.
struct ReplyClaimHint: View {
    @Environment(Session.self) private var session
    let sharedConv: String?
    @State private var hint: TeamWire.ReplyHint?

    var body: some View {
        // a stack, not a Group: a Group hands its modifiers to its
        // children, and with no hint yet there is no child to run `.task`
        VStack(spacing: 0) {
            switch hint {
            case .writing(let name):
                Label("\(name) is writing a reply", systemImage: "pencil")
            case .assigned(let name):
                Label("Assigned to \(name)", systemImage: "person")
            case nil:
                EmptyView()
            }
        }
        .font(.caption)
        .foregroundStyle(.orange)
        // one sentence, not an icon and a line read apart
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("reply.claimHint")
        .task(id: sharedConv) {
            guard let c = TeamWire.Conv(sharedConv) else { return }
            hint = await session.replyHint(c)
        }
    }
}
