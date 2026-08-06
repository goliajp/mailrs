import SwiftUI

struct ConversationListView: View {
    @Environment(Session.self) private var session

    var body: some View {
        NavigationStack {
            List(session.conversations) { conversation in
                ConversationRow(conversation: conversation)
            }
            .listStyle(.plain)
            .refreshable { await session.loadConversations() }
            .navigationTitle("Inbox")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Sign out") { session.signOut() }
                }
            }
            .overlay {
                if session.conversations.isEmpty {
                    ContentUnavailableView("All caught up", systemImage: "tray")
                }
            }
        }
    }
}

struct ConversationRow: View {
    let conversation: Wire.Conversation

    private var sender: String {
        conversation.participants.first ?? "(unknown)"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            // The unread marker is a dot rather than bold-everything:
            // one signal, in one place, that survives a long subject.
            Circle()
                .fill(conversation.unreadCount > 0 ? Color.accentColor : .clear)
                .frame(width: 8, height: 8)
                .padding(.top, 6)

            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(sender)
                        .font(.subheadline.weight(conversation.unreadCount > 0 ? .semibold : .regular))
                        .lineLimit(1)
                    Spacer()
                    Text(Date(timeIntervalSince1970: TimeInterval(conversation.lastDate)),
                         format: .dateTime.month().day())
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(conversation.subject.isEmpty ? "(no subject)" : conversation.subject)
                    .font(.subheadline)
                    .lineLimit(1)
                Text(conversation.snippet)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }
}
