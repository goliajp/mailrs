import SwiftUI

/// Over the Inbox: the whole Inbox or one of its sections, and in New a
/// way to accept the senders shown. The same choice as the web's section
/// bar, on a phone's width.
struct InboxSectionBar: View {
    @Environment(Session.self) private var session

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                chip("All", section: nil, id: "section.all")
                ForEach(InboxSection.allCases) { section in
                    chip(section.label, section: section, id: "section.\(section.rawValue)")
                }
                if session.inboxSection == .new, !session.conversations.isEmpty {
                    Button("Accept all") { Task { await session.acceptShownSenders() } }
                        .font(.footnote)
                        .accessibilityIdentifier("section.acceptAll")
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }

    private func chip(_ title: LocalizedStringKey, section: InboxSection?, id: String) -> some View {
        let chosen = session.inboxSection == section
        return Button { Task { await session.selectSection(section) } } label: {
            Text(title)
                .font(.footnote)
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Capsule().fill(Self.fill(chosen)))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
        .accessibilityAddTraits(Self.traits(chosen))
    }

    private static func fill(_ chosen: Bool) -> Color {
        if chosen { return Color.accentColor.opacity(0.18) }
        return Color.clear
    }

    private static func traits(_ chosen: Bool) -> AccessibilityTraits {
        if chosen { return .isSelected }
        return []
    }
}
