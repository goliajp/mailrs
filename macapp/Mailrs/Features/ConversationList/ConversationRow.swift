import SwiftUI

struct ConversationRow: View {
    let conversation: ConversationSummary

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            unreadDot
            avatar

            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline) {
                    Text(conversation.last_sender.extractedName)
                        .font(.subheadline.weight(conversation.isUnread ? .semibold : .regular))
                        .lineLimit(1)
                    if conversation.message_count > 1 {
                        Text("\(conversation.message_count)")
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(relativeDate(conversation.last_date))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }

                Text(conversation.subject.isEmpty ? "(无主题)" : conversation.subject)
                    .font(.subheadline.weight(conversation.isUnread ? .medium : .regular))
                    .lineLimit(1)

                Text(conversation.snippet)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 6) {
                    if conversation.pinned {
                        badge("已置顶", systemImage: "pin.fill", tint: .orange)
                    }
                    if conversation.flagged {
                        badge("星标", systemImage: "star.fill", tint: .yellow)
                    }
                    if conversation.requires_action {
                        badge("待处理", systemImage: "checkmark.circle", tint: .blue)
                    }
                    if let cat = MailCategory(rawValue: conversation.category), cat != .personal {
                        badge(cat.displayName, systemImage: cat.systemImage, tint: categoryTint(cat))
                    }
                }
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    private var unreadDot: some View {
        Circle()
            .fill(conversation.isUnread ? Color.accentColor : Color.clear)
            .frame(width: 8, height: 8)
            .padding(.top, 6)
    }

    private var avatar: some View {
        Circle()
            .fill(Color.secondary.opacity(0.15))
            .frame(width: 32, height: 32)
            .overlay(
                Text(conversation.last_sender.avatarInitial)
                    .font(.caption.bold())
                    .foregroundStyle(.secondary)
            )
    }

    private func badge(_ text: String, systemImage: String, tint: Color) -> some View {
        HStack(spacing: 3) {
            Image(systemName: systemImage).font(.system(size: 9))
            Text(text).font(.system(size: 10))
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(tint.opacity(0.15))
        .foregroundStyle(tint)
        .clipShape(Capsule())
    }

    private func categoryTint(_ category: MailCategory) -> Color {
        switch category {
        case .personal: return .blue
        case .promotion: return .pink
        case .notification: return .cyan
        case .general: return .secondary
        case .spam: return .orange
        case .scam: return .red
        }
    }

    private func relativeDate(_ date: Date) -> String {
        let now = Date()
        let diff = now.timeIntervalSince(date)
        if diff < 60 { return "刚刚" }
        let cal = Calendar.current
        if cal.isDateInToday(date) {
            let f = DateFormatter(); f.dateFormat = "HH:mm"
            return f.string(from: date)
        }
        if cal.isDateInYesterday(date) { return "昨天" }
        if diff < 7 * 24 * 3600 {
            let f = DateFormatter(); f.dateFormat = "EEE"; f.locale = Locale(identifier: "zh_CN")
            return f.string(from: date)
        }
        if cal.component(.year, from: now) == cal.component(.year, from: date) {
            let f = DateFormatter(); f.dateFormat = "M月d日"
            return f.string(from: date)
        }
        let f = DateFormatter(); f.dateFormat = "yyyy/M/d"
        return f.string(from: date)
    }
}

private extension String {
    /// Parses "Name <email>" → "Name"; falls back to email local-part.
    var extractedName: String {
        if let lt = firstIndex(of: "<") {
            let name = self[..<lt].trimmingCharacters(in: .whitespacesAndNewlines)
            if !name.isEmpty { return name.trimmingCharacters(in: CharacterSet(charactersIn: "\"")) }
        }
        if let at = firstIndex(of: "@") {
            return String(self[..<at])
        }
        return self
    }

    var avatarInitial: String {
        let name = extractedName
        return String(name.first ?? Character("?")).uppercased()
    }
}
